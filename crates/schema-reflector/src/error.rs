use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReflectError {
    #[error("postgres error: {0}")]
    Postgres(#[from] tokio_postgres::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

pub type Result<T> = std::result::Result<T, ReflectError>;
