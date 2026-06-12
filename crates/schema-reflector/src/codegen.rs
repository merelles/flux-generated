use std::fs;
use std::path::Path;

use convert_case::{Case, Casing};

use crate::error::Result;
use crate::model::{ColumnInfo, DatabaseSchema, SchemaInfo, TableInfo};

pub fn write_entities(schema: &DatabaseSchema, output_dir: impl AsRef<Path>) -> Result<()> {
    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir)?;

    for schema_info in &schema.schemas {
        write_schema_module(schema_info, output_dir)?;
    }

    write_root_mod_file(schema, output_dir)?;
    Ok(())
}

pub fn write_modules(schema: &DatabaseSchema, output_dir: impl AsRef<Path>) -> Result<()> {
    write_entities(schema, output_dir)
}

fn write_schema_module(schema: &SchemaInfo, output_dir: &Path) -> Result<()> {
    let schema_dir = output_dir.join(schema.name.to_case(Case::Snake));
    fs::create_dir_all(&schema_dir)?;

    for table in &schema.tables {
        let file_name = format!("{}.rs", table.name.to_case(Case::Snake));
        fs::write(schema_dir.join(file_name), render_table_module(table))?;
    }

    fs::write(schema_dir.join("mod.rs"), render_schema_mod(schema))?;
    Ok(())
}

fn write_root_mod_file(schema: &DatabaseSchema, output_dir: &Path) -> Result<()> {
    let mut output = String::new();
    for schema_info in &schema.schemas {
        let module_name = schema_info.name.to_case(Case::Snake);
        output.push_str(&format!("pub mod {};\n", module_name));
        output.push_str(&format!("pub use {}::*;\n", module_name));
    }
    fs::write(output_dir.join("mod.rs"), output)?;
    Ok(())
}

fn render_schema_mod(schema: &SchemaInfo) -> String {
    let mut output = String::new();
    for table in &schema.tables {
        let module_name = table.name.to_case(Case::Snake);
        let struct_name = table.name.to_case(Case::Pascal);
        output.push_str(&format!("pub mod {};\n", module_name));
        output.push_str(&format!("pub use {}::{};\n", module_name, struct_name));
    }
    output
}

fn render_table_module(table: &TableInfo) -> String {
    let struct_name = table.name.to_case(Case::Pascal);

    let mut output = String::new();
    output.push_str("use serde::{Deserialize, Serialize};\n\n");
    output.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
    output.push_str(&format!("pub struct {} {{\n", struct_name));

    for column in &table.columns {
        let field_name = sanitize_field_name(&column.name);
        let field_type = rust_type_for_column(column);
        output.push_str(&format!("    pub {}: {},\n", field_name, field_type));
    }

    output.push_str("}\n\n");
    output.push_str(&format!("impl {} {{\n", struct_name));
    output.push_str(&format!(
        "    pub const SCHEMA_NAME: &'static str = \"{}\";\n",
        table.schema
    ));
    output.push_str(&format!(
        "    pub const TABLE_NAME: &'static str = \"{}\";\n",
        table.name
    ));
    output.push_str("}\n");

    output
}

fn rust_type_for_column(column: &ColumnInfo) -> String {
    let mapped = map_postgres_type(&column.udt_name);
    if column.is_nullable {
        format!("Option<{}>", mapped)
    } else {
        mapped
    }
}

fn map_postgres_type(udt_name: &str) -> String {
    match udt_name {
        "int2" => "i16".to_string(),
        "int4" => "i32".to_string(),
        "int8" => "i64".to_string(),
        "float4" => "f32".to_string(),
        "float8" => "f64".to_string(),
        "bool" => "bool".to_string(),
        "varchar" | "bpchar" | "text" | "json" | "jsonb" | "uuid" | "date" | "timestamp"
        | "timestamptz" | "time" | "timetz" | "bytea" | "numeric" | "money" => {
            "String".to_string()
        }
        "int2[]" | "int4[]" | "int8[]" | "varchar[]" | "text[]" | "uuid[]" => {
            "Vec<String>".to_string()
        }
        other if other.ends_with("[]") => "Vec<String>".to_string(),
        _ => "String".to_string(),
    }
}

fn sanitize_field_name(name: &str) -> String {
    let candidate = name.to_case(Case::Snake);
    if matches!(
        candidate.as_str(),
        "type" | "match" | "ref" | "self" | "crate" | "super" | "mod"
    ) {
        format!("{candidate}_field")
    } else {
        candidate
    }
}
