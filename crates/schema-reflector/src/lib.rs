mod aggregate;
mod api;
mod aggregate_code;
mod codegen;
mod error;
mod introspect;
mod model;

pub use aggregate::{
    build_aggregate_manifests, write_aggregate_manifests, AggregateChild, AggregateManifest,
    AggregateRoot,
};
pub use api::write_api_code;
pub use aggregate_code::write_aggregate_code;
pub use codegen::{write_entities, write_modules};
pub use error::{ReflectError, Result};
pub use introspect::{reflect_database, reflect_database_from_url};
pub use model::{
    ColumnInfo, DatabaseSchema, ForeignKeyInfo, GeneratedModule, RustField, RustStruct, TableInfo,
};
