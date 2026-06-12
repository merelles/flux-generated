use std::env;
use std::fs;
use std::path::PathBuf;

use schema_reflector::{
    reflect_database_from_url, write_aggregate_code, write_aggregate_manifests, write_api_code,
    write_entities,
};

fn clean_dir(path: &PathBuf) -> std::io::Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    fs::create_dir_all(path)?;
    Ok(())
}

fn write_generated_root_mod(root_dir: &PathBuf) -> std::io::Result<()> {
    let content = r#"pub mod entities;
pub mod aggregates;
pub mod api;
"#;

    fs::write(root_dir.join("mod.rs"), content)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();

    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let entities_dir = env::var("ENTITIES_OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("generated/entities"));
    let docs_dir = env::var("DOCS_OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("generated/docs"));
    let aggregates_dir = env::var("AGGREGATES_OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("generated/aggregates"));
    let generated_root = entities_dir
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("generated"));

    let schema = reflect_database_from_url(&database_url).await?;
    clean_dir(&entities_dir)?;
    clean_dir(&docs_dir)?;
    clean_dir(&aggregates_dir)?;
    clean_dir(&generated_root.join("api"))?;

    write_entities(&schema, &entities_dir)?;
    write_aggregate_manifests(&schema, &docs_dir)?;
    write_aggregate_code(&schema, &aggregates_dir)?;
    write_api_code(&generated_root, include_str!("../api_template.rs"))?;
    write_generated_root_mod(&generated_root)?;

    println!("entities generated in {}", entities_dir.display());
    println!("docs generated in {}", docs_dir.display());
    println!("aggregate code generated in {}", aggregates_dir.display());
    Ok(())
}
