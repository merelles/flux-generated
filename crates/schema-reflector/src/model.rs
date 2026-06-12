use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSchema {
    pub schemas: Vec<SchemaInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaInfo {
    pub name: String,
    pub tables: Vec<TableInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub ordinal_position: i32,
    pub data_type: String,
    pub udt_name: String,
    pub is_nullable: bool,
    pub default_value: Option<String>,
    pub is_primary_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeyInfo {
    pub name: String,
    pub column: String,
    pub foreign_schema: String,
    pub foreign_table: String,
    pub foreign_column: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationInfo {
    pub name: String,
    pub local_column: String,
    pub foreign_schema: String,
    pub foreign_table: String,
    pub foreign_column: String,
}

#[derive(Debug, Clone)]
pub struct GeneratedModule {
    pub schema: String,
    pub code: String,
}

#[derive(Debug, Clone)]
pub struct RustStruct {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub fields: Vec<RustField>,
    pub relations: Vec<RelationInfo>,
}

#[derive(Debug, Clone)]
pub struct RustField {
    pub name: String,
    pub ty: String,
    pub is_optional: bool,
}
