use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use convert_case::{Case, Casing};

use crate::aggregate::{build_aggregate_manifests, AggregateManifest};
use crate::error::Result;
use crate::model::{ColumnInfo, DatabaseSchema, TableInfo};

pub fn write_aggregate_code(schema: &DatabaseSchema, output_dir: impl AsRef<Path>) -> Result<()> {
    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir)?;

    let manifests = build_aggregate_manifests(schema);
    let table_index = index_tables(schema);

    for manifest in &manifests {
        write_aggregate_module(manifest, &table_index, output_dir)?;
    }

    write_root_mod_file(&manifests, output_dir)?;
    Ok(())
}

fn write_aggregate_module(
    manifest: &AggregateManifest,
    table_index: &BTreeMap<(String, String), TableInfo>,
    output_dir: &Path,
) -> Result<()> {
    let schema_dir = output_dir.join(manifest.root.schema.to_case(Case::Snake));
    fs::create_dir_all(&schema_dir)?;

    let file_name = format!("{}.rs", manifest.root.table.to_case(Case::Snake));
    fs::write(schema_dir.join(file_name), render_aggregate_module(manifest, table_index))?;
    Ok(())
}

fn write_root_mod_file(manifests: &[AggregateManifest], output_dir: &Path) -> Result<()> {
    let mut schemas: BTreeMap<String, Vec<&AggregateManifest>> = BTreeMap::new();
    for manifest in manifests {
        schemas
            .entry(manifest.root.schema.clone())
            .or_default()
            .push(manifest);
    }

    let mut root_output = String::new();

    for (schema_name, schema_manifests) in schemas {
        let schema_dir = output_dir.join(schema_name.to_case(Case::Snake));
        let mut output = String::new();
        for manifest in schema_manifests {
            let module_name = manifest.root.table.to_case(Case::Snake);
            let aggregate_name = manifest.root.table.to_case(Case::Pascal);
            output.push_str(&format!("pub mod {};\n", module_name));
            output.push_str(&format!(
                "pub use {}::{{{}Aggregate, {}AggregateCommand, {}AggregateExecutor, {}AggregateRepository, {}Command}};\n",
                module_name,
                aggregate_name,
                aggregate_name,
                aggregate_name,
                aggregate_name,
                aggregate_name
            ));
        }
        fs::write(schema_dir.join("mod.rs"), output)?;

        root_output.push_str(&format!("pub mod {};\n", schema_name.to_case(Case::Snake)));
        root_output.push_str(&format!("pub use {}::*;\n\n", schema_name.to_case(Case::Snake)));
    }

    fs::write(output_dir.join("mod.rs"), root_output)?;
    Ok(())
}

fn render_aggregate_module(
    manifest: &AggregateManifest,
    table_index: &BTreeMap<(String, String), TableInfo>,
) -> String {
    let root_table = table_index
        .get(&(manifest.root.schema.clone(), manifest.root.table.clone()))
        .expect("root table must exist");

    let aggregate_name = manifest.root.table.to_case(Case::Pascal);
    let root_entity_name = aggregate_name.clone();
    let root_entity_import = entity_import_path(&manifest.root.schema, &manifest.root.table);
    let root_command_name = format!("{}Command", aggregate_name);
    let aggregate_command_name = format!("{}AggregateCommand", aggregate_name);
    let aggregate_response_name = format!("{}Aggregate", aggregate_name);
    let repository_trait_name = format!("{}AggregateRepository", aggregate_name);
    let executor_name = format!("{}AggregateExecutor", aggregate_name);
    let root_mapper_name = format!("map_{}_row", manifest.root.table.to_case(Case::Snake));
    let root_insert_name = format!("insert_{}", manifest.root.table.to_case(Case::Snake));
    let root_pk_column = primary_key_column(root_table).expect("root table must have a primary key");
    let root_pk_field = sanitize_field_name(&root_pk_column.name);
    let root_pk_type = rust_type_for_column(root_pk_column);

    let mut used_field_names: BTreeSet<String> = BTreeSet::new();
    let mut used_command_names: BTreeSet<String> = BTreeSet::new();
    let mut used_mapper_names: BTreeSet<String> = BTreeSet::new();
    let mut used_insert_names: BTreeSet<String> = BTreeSet::new();

    used_command_names.insert(root_command_name.clone());
    used_mapper_names.insert(root_mapper_name.clone());
    used_insert_names.insert(root_insert_name.clone());

    let mut child_table_counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for child in &manifest.children {
        *child_table_counts
            .entry((child.schema.clone(), child.table.clone()))
            .or_insert(0) += 1;
    }

    let mut seen_children: BTreeSet<(String, String, String, String, String, String)> = BTreeSet::new();
    let mut child_specs = Vec::new();
    for child in &manifest.children {
        let child_key = (
            child.schema.clone(),
            child.table.clone(),
            child.foreign_key.clone(),
            child.parent_schema.clone(),
            child.parent_table.clone(),
            child.parent_key.clone(),
        );
        if !seen_children.insert(child_key) {
            continue;
        }

        if let Some(child_table) = table_index.get(&(child.schema.clone(), child.table.clone())) {
            let child_entity_name = child_table.name.to_case(Case::Pascal);
            let child_entity_import = entity_import_path(&child.schema, &child.table);
            let child_table_name = child_table.name.to_case(Case::Snake);
            let child_name_count = child_table_counts
                .get(&(child.schema.clone(), child.table.clone()))
                .copied()
                .unwrap_or(1);
            let relation_suffix = sanitize_field_name(&child.foreign_key);
            let relation_suffix_pascal = relation_suffix.to_case(Case::Pascal);
            let same_table_as_root =
                child.schema == manifest.root.schema && child.table == manifest.root.table;

            let field_name = if child_name_count > 1 || same_table_as_root {
                ensure_unique_name(
                    format!("{}_{}", child_table_name, relation_suffix),
                    None,
                    &mut used_field_names,
                )
            } else {
                ensure_unique_name(
                    child_table_name.clone(),
                    Some(format!("{}_{}", child_table_name, relation_suffix)),
                    &mut used_field_names,
                )
            };
            let child_command_name = if child_name_count > 1 || same_table_as_root {
                ensure_unique_name(
                    format!("{}{}Command", child_entity_name, relation_suffix_pascal),
                    None,
                    &mut used_command_names,
                )
            } else {
                ensure_unique_name(
                    format!("{}Command", child_entity_name),
                    Some(format!("{}{}Command", child_entity_name, relation_suffix_pascal)),
                    &mut used_command_names,
                )
            };
            let child_mapper_name = if child_name_count > 1 || same_table_as_root {
                ensure_unique_name(
                    format!("map_{}_{}_row", child_table_name, relation_suffix),
                    None,
                    &mut used_mapper_names,
                )
            } else {
                ensure_unique_name(
                    format!("map_{}_row", child_table_name),
                    Some(format!("map_{}_{}_row", child_table_name, relation_suffix)),
                    &mut used_mapper_names,
                )
            };
            let child_insert_name = if child_name_count > 1 || same_table_as_root {
                ensure_unique_name(
                    format!("insert_{}_{}", child_table_name, relation_suffix),
                    None,
                    &mut used_insert_names,
                )
            } else {
                ensure_unique_name(
                    format!("insert_{}", child_table_name),
                    Some(format!("insert_{}_{}", child_table_name, relation_suffix)),
                    &mut used_insert_names,
                )
            };
            child_specs.push(ChildSpec {
                table: child_table,
                field_name,
                entity_name: child_entity_name,
                entity_import: child_entity_import,
                command_name: child_command_name,
                mapper_name: child_mapper_name,
                insert_name: child_insert_name,
                foreign_key: child.foreign_key.clone(),
            });
        }
    }

    let mut output = String::new();
    output.push_str("use async_trait::async_trait;\n");
    output.push_str("use serde::{Deserialize, Serialize};\n");
    output.push_str("use std::sync::Arc;\n");
    output.push_str("use tokio::sync::Mutex;\n");
    output.push_str("use tokio_postgres::types::ToSql;\n");
    output.push_str("use tokio_postgres::{Client, Row, Transaction};\n");
    output.push('\n');
    let mut entity_imports: BTreeSet<String> = BTreeSet::new();
    entity_imports.insert(root_entity_import);
    for spec in &child_specs {
        entity_imports.insert(spec.entity_import.clone());
    }
    for entity_import in entity_imports {
        output.push_str(&format!("use {};\n", entity_import));
    }
    output.push('\n');
    output.push_str("pub type AggregateWriteError = Box<dyn std::error::Error + Send + Sync>;\n\n");

    output.push_str(&format!(
        "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct {} {{\n",
        root_command_name
    ));
    output.push_str(&render_command_fields(root_table, None));
    output.push_str("}\n\n");
    output.push_str(&render_row_mapper(root_table, &root_mapper_name, &root_entity_name));
    output.push('\n');

    for spec in &child_specs {
        output.push_str(&format!(
            "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct {} {{\n",
            spec.command_name
        ));
        output.push_str(&render_command_fields(spec.table, Some(spec.foreign_key.as_str())));
        output.push_str("}\n\n");
        output.push_str(&render_row_mapper(spec.table, &spec.mapper_name, &spec.entity_name));
        output.push('\n');
    }

    output.push_str(&format!(
        "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct {} {{\n",
        aggregate_command_name
    ));
    output.push_str(&format!("    pub root: {},\n", root_command_name));
    for spec in &child_specs {
        output.push_str(&format!(
            "    pub {}: Vec<{}>,\n",
            spec.field_name,
            spec.command_name
        ));
    }
    output.push_str("}\n\n");

    output.push_str(&format!(
        "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct {} {{\n",
        aggregate_response_name
    ));
    output.push_str(&format!("    pub root: {},\n", root_entity_name));
    for spec in &child_specs {
        output.push_str(&format!(
            "    pub {}: Vec<{}>,\n",
            spec.field_name,
            spec.entity_name
        ));
    }
    output.push_str("}\n\n");

    output.push_str(&format!("impl {} {{\n", aggregate_response_name));
    let params = child_specs
        .iter()
        .map(|spec| format!("{}: Vec<{}>", spec.field_name, spec.entity_name))
        .collect::<Vec<_>>()
        .join(", ");
    if params.is_empty() {
        output.push_str(&format!("    pub fn new(root: {}) -> Self {{\n", root_entity_name));
    } else {
        output.push_str(&format!("    pub fn new(root: {}, {}) -> Self {{\n", root_entity_name, params));
    }
    output.push_str("        Self {\n");
    output.push_str("            root,\n");
    for spec in &child_specs {
        output.push_str(&format!("            {},\n", spec.field_name));
    }
    output.push_str("        }\n");
    output.push_str("    }\n");
    output.push_str("}\n\n");

    output.push_str(&format!("#[async_trait]\npub trait {} {{\n", repository_trait_name));
    output.push_str(&format!(
        "    async fn upsert_one(&self, command: {}) -> Result<{}, AggregateWriteError>;\n",
        aggregate_command_name, aggregate_response_name
    ));
    output.push_str(&format!(
        "    async fn upsert_many(&self, commands: Vec<{}>) -> Result<Vec<{}>, AggregateWriteError>;\n",
        aggregate_command_name, aggregate_response_name
    ));
    output.push_str("}\n\n");

    output.push_str(&format!(
        "pub struct {} {{\n    client: Arc<Mutex<Client>>,\n}}\n\n",
        executor_name
    ));
    output.push_str(&format!("impl {} {{\n", executor_name));
    output.push_str("    pub fn new(client: Arc<Mutex<Client>>) -> Self {\n");
    output.push_str("        Self { client }\n");
    output.push_str("    }\n\n");
    output.push_str(&format!(
        "    async fn {}(tx: &Transaction<'_>, command: &{}) -> Result<{}, AggregateWriteError> {{\n",
        root_insert_name, root_command_name, root_entity_name
    ));
    output.push_str(&render_insert_body(root_table, None, &root_mapper_name));
    output.push_str("    }\n\n");
    for spec in &child_specs {
        output.push_str(&format!(
            "    async fn {}(tx: &Transaction<'_>, parent_id: {}, command: &{}) -> Result<{}, AggregateWriteError> {{\n",
            spec.insert_name, root_pk_type, spec.command_name, spec.entity_name
        ));
        output.push_str(&render_insert_body(
            spec.table,
            Some(spec.foreign_key.as_str()),
            &spec.mapper_name,
        ));
        output.push_str("    }\n\n");
    }
    output.push_str("}\n\n");

    output.push_str(&format!(
        "#[async_trait]\nimpl {} for {} {{\n",
        repository_trait_name, executor_name
    ));
    output.push_str(&format!(
        "    async fn upsert_one(&self, command: {}) -> Result<{}, AggregateWriteError> {{\n",
        aggregate_command_name, aggregate_response_name
    ));
    output.push_str("        let mut client = self.client.lock().await;\n");
    output.push_str("        let tx = client.transaction().await?;\n");
    output.push_str(&format!(
        "        let root = Self::{}(&tx, &command.root).await?;\n",
        root_insert_name
    ));
    for spec in &child_specs {
        let items_var = format!("{}_items", spec.field_name);
        output.push_str(&format!(
            "        let mut {} = Vec::with_capacity(command.{}.len());\n",
            items_var, spec.field_name
        ));
        output.push_str(&format!("        for item in command.{} {{\n", spec.field_name));
        output.push_str(&format!(
            "            {}.push(Self::{}(&tx, root.{}.clone(), &item).await?);\n",
            items_var, spec.insert_name, root_pk_field
        ));
        output.push_str("        }\n");
    }
    output.push_str("        tx.commit().await?;\n");
    if child_specs.is_empty() {
        output.push_str(&format!("        Ok({}::new(root))\n", aggregate_response_name));
    } else {
        output.push_str(&format!("        Ok({}::new(root", aggregate_response_name));
        for spec in &child_specs {
            output.push_str(&format!(", {}_items", spec.field_name));
        }
        output.push_str("))\n");
    }
    output.push_str("    }\n\n");
    output.push_str(&format!(
        "    async fn upsert_many(&self, commands: Vec<{}>) -> Result<Vec<{}>, AggregateWriteError> {{\n",
        aggregate_command_name, aggregate_response_name
    ));
    output.push_str("        let mut client = self.client.lock().await;\n");
    output.push_str("        let tx = client.transaction().await?;\n");
    output.push_str("        let mut aggregates = Vec::with_capacity(commands.len());\n");
    output.push_str("        for command in commands {\n");
    output.push_str(&format!(
        "            let root = Self::{}(&tx, &command.root).await?;\n",
        root_insert_name
    ));
    for spec in &child_specs {
        let items_var = format!("{}_items", spec.field_name);
        output.push_str(&format!(
            "            let mut {} = Vec::with_capacity(command.{}.len());\n",
            items_var, spec.field_name
        ));
        output.push_str(&format!("            for item in command.{} {{\n", spec.field_name));
        output.push_str(&format!(
            "                {}.push(Self::{}(&tx, root.{}.clone(), &item).await?);\n",
            items_var, spec.insert_name, root_pk_field
        ));
        output.push_str("            }\n");
    }
    if child_specs.is_empty() {
        output.push_str(&format!("            aggregates.push({}::new(root));\n", aggregate_response_name));
    } else {
        output.push_str(&format!("            aggregates.push({}::new(root", aggregate_response_name));
        for spec in &child_specs {
            output.push_str(&format!(", {}_items", spec.field_name));
        }
        output.push_str("));\n");
    }
    output.push_str("        }\n");
    output.push_str("        tx.commit().await?;\n");
    output.push_str("        Ok(aggregates)\n");
    output.push_str("    }\n");
    output.push_str("}\n");

    output
}

fn render_command_fields(table: &TableInfo, parent_fk: Option<&str>) -> String {
    let mut output = String::new();
    if let Some(pk) = primary_key_column(table) {
        let field_name = sanitize_field_name(&pk.name);
        output.push_str(&format!(
            "    pub {}: Option<{}>,\n",
            field_name,
            rust_type_for_column(pk)
        ));
    }
    for column in &table.columns {
        if column.is_primary_key {
            continue;
        }
        if parent_fk.map(|fk| fk == column.name).unwrap_or(false) {
            continue;
        }
        let field_name = sanitize_field_name(&column.name);
        output.push_str(&format!(
            "    pub {}: {},\n",
            field_name,
            rust_type_for_command(column)
        ));
    }
    output
}

fn render_insert_body(
    table: &TableInfo,
    parent_fk: Option<&str>,
    mapper_name: &str,
) -> String {
    let returning = table
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let pk = primary_key_column(table);
    let pk_field = pk.map(|column| sanitize_field_name(&column.name));
    let mut insert_columns = Vec::new();
    let mut insert_placeholders = Vec::new();
    let mut insert_params = Vec::new();
    let mut update_sets = Vec::new();

    if let Some(parent_fk) = parent_fk {
        insert_columns.push(parent_fk.to_string());
        insert_placeholders.push(format!("${}", insert_placeholders.len() + 1));
        insert_params.push("&parent_id".to_string());
        update_sets.push(format!("{parent_fk} = EXCLUDED.{parent_fk}"));
    }

    for column in &table.columns {
        if column.is_primary_key || parent_fk.map(|fk| fk == column.name).unwrap_or(false) {
            continue;
        }
        insert_columns.push(column.name.clone());
        insert_placeholders.push(format!("${}", insert_placeholders.len() + 1));
        insert_params.push(format!("&command.{}", sanitize_field_name(&column.name)));
        update_sets.push(format!(
            "{} = EXCLUDED.{}",
            column.name, column.name
        ));
    }

    let mut output = String::new();
    if let (Some(pk), Some(pk_field)) = (pk, pk_field) {
        let update_clause = update_sets.join(", ");

        let mut columns_with_pk = vec![pk.name.clone()];
        columns_with_pk.extend(insert_columns.iter().cloned());

        let mut placeholders_with_pk = vec!["$1".to_string()];
        for index in 0..insert_placeholders.len() {
            placeholders_with_pk.push(format!("${}", index + 2));
        }

        let mut params_with_pk = vec!["id_value".to_string()];
        params_with_pk.extend(insert_params.iter().cloned());

        output.push_str(&format!(
            "        if let Some(id_value) = command.{}.as_ref() {{\n",
            pk_field
        ));
        output.push_str(&format!(
            "            let params: &[&(dyn ToSql + Sync)] = &[{}];\n",
            params_with_pk
                .iter()
                .map(|param| {
                    if param == "id_value" {
                        param.clone()
                    } else {
                        format!("&{}", param)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        ));
        output.push_str(&format!(
            "            let row = tx.query_one(\"INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {} RETURNING {}\", params).await?;\n",
            table.name,
            columns_with_pk.join(", "),
            placeholders_with_pk.join(", "),
            pk.name,
            update_clause,
            returning
        ));
        output.push_str(&format!("            Ok({}(row))\n", mapper_name));
        output.push_str("        } else {\n");
        if insert_params.is_empty() {
            output.push_str(&format!(
                "            let row = tx.query_one(\"INSERT INTO {} ({}) VALUES ({}) RETURNING {}\", &[]).await?;\n",
                table.name,
                insert_columns.join(", "),
                insert_placeholders.join(", "),
                returning
            ));
        } else {
            output.push_str(&format!(
                "            let params: &[&(dyn ToSql + Sync)] = &[{}];\n",
                insert_params
                    .iter()
                    .map(|param| format!("&{}", param))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            output.push_str(&format!(
                "            let row = tx.query_one(\"INSERT INTO {} ({}) VALUES ({}) RETURNING {}\", params).await?;\n",
                table.name,
                insert_columns.join(", "),
                insert_placeholders.join(", "),
                returning
            ));
        }
        output.push_str(&format!("            Ok({}(row))\n", mapper_name));
        output.push_str("        }\n");
    } else if insert_params.is_empty() {
        output.push_str(&format!(
            "        let row = tx.query_one(\"INSERT INTO {} ({}) VALUES ({}) RETURNING {}\", &[]).await?;\n",
            table.name,
            insert_columns.join(", "),
            insert_placeholders.join(", "),
            returning
        ));
        output.push_str(&format!("        Ok({}(row))\n", mapper_name));
    } else {
        output.push_str(&format!(
            "        let params: &[&(dyn ToSql + Sync)] = &[{}];\n",
            insert_params
                .iter()
                .map(|param| format!("&{}", param))
                .collect::<Vec<_>>()
                .join(", ")
                .replace("&&", "&")
        ));
        output.push_str(&format!(
            "        let row = tx.query_one(\"INSERT INTO {} ({}) VALUES ({}) RETURNING {}\", params).await?;\n",
            table.name,
            insert_columns.join(", "),
            insert_placeholders.join(", "),
            returning
        ));
        output.push_str(&format!("        Ok({}(row))\n", mapper_name));
    }
    output
}

fn render_row_mapper(table: &TableInfo, mapper_name: &str, entity_name: &str) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "fn {}(row: Row) -> {} {{\n",
        mapper_name, entity_name
    ));
    output.push_str(&format!("    {} {{\n", entity_name));
    for column in &table.columns {
        let field_name = sanitize_field_name(&column.name);
        output.push_str(&format!(
            "        {}: row.get(\"{}\"),\n",
            field_name, column.name
        ));
    }
    output.push_str("    }\n");
    output.push_str("}\n");
    output
}

fn entity_import_path(schema: &str, table: &str) -> String {
    format!(
        "crate::entities::{}::{}",
        schema.to_case(Case::Snake),
        table.to_case(Case::Pascal)
    )
}

fn rust_type_for_column(column: &ColumnInfo) -> String {
    let mapped = map_postgres_type(&column.udt_name);
    if column.is_nullable {
        format!("Option<{}>", mapped)
    } else {
        mapped
    }
}

fn rust_type_for_command(column: &ColumnInfo) -> String {
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

fn ensure_unique_name(
    base: String,
    fallback: Option<String>,
    used: &mut BTreeSet<String>,
) -> String {
    if used.insert(base.clone()) {
        return base;
    }

    if let Some(fallback) = fallback {
        if used.insert(fallback.clone()) {
            return fallback;
        }
    }

    let mut index = 2usize;
    loop {
        let candidate = format!("{}_{}", base, index);
        if used.insert(candidate.clone()) {
            return candidate;
        }
        index += 1;
    }
}

fn primary_key_column(table: &TableInfo) -> Option<&ColumnInfo> {
    table.columns.iter().find(|column| column.is_primary_key)
}

fn index_tables(schema: &DatabaseSchema) -> BTreeMap<(String, String), TableInfo> {
    let mut tables = BTreeMap::new();
    for schema_info in &schema.schemas {
        for table in &schema_info.tables {
            tables.insert((table.schema.clone(), table.name.clone()), table.clone());
        }
    }
    tables
}

struct ChildSpec<'a> {
    table: &'a TableInfo,
    field_name: String,
    entity_name: String,
    entity_import: String,
    command_name: String,
    mapper_name: String,
    insert_name: String,
    foreign_key: String,
}
