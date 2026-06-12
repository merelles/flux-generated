#[path = "../generated/mod.rs"]
mod generated;

pub use generated::{aggregates, entities};

use std::env;
use std::error::Error;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_postgres::NoTls;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();

    let database_url = env::var("DATABASE_URL")?;
    let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_string());

    let (client, connection) = tokio_postgres::connect(&database_url, NoTls).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("postgres connection error: {error}");
        }
    });

    let state = generated::api::AppState::new(Arc::new(client)).await?;
    let app = generated::api::build_router(state);

    let listener = TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
