use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use convert_case::{Case, Casing};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::model::{DatabaseSchema, TableInfo};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateManifest {
    pub name: String,
    pub root: AggregateRoot,
    pub children: Vec<AggregateChild>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateRoot {
    pub schema: String,
    pub table: String,
    pub primary_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateChild {
    pub schema: String,
    pub table: String,
    pub foreign_key: String,
    pub parent_schema: String,
    pub parent_table: String,
    pub parent_key: String,
}

pub fn write_aggregate_manifests(schema: &DatabaseSchema, output_dir: impl AsRef<Path>) -> Result<()> {
    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir)?;

    for manifest in build_aggregate_manifests(schema) {
        let schema_dir = output_dir.join(manifest.root.schema.to_case(Case::Snake));
        fs::create_dir_all(&schema_dir)?;
        let file_name = format!("{}.yml", manifest.root.table.to_case(Case::Snake));
        let yaml = serde_yaml::to_string(&manifest)?;
        fs::write(schema_dir.join(file_name), yaml)?;
    }

    Ok(())
}

pub fn build_aggregate_manifests(schema: &DatabaseSchema) -> Vec<AggregateManifest> {
    let table_index = index_tables(schema);
    let inbound_relations = build_inbound_relations(schema);

    let mut manifests = Vec::new();

    for schema_info in &schema.schemas {
        for table in &schema_info.tables {
            let inbound = inbound_relations
                .get(&(table.schema.clone(), table.name.clone()))
                .cloned()
                .unwrap_or_default();

            if inbound.is_empty() {
                continue;
            }

            let primary_keys = table
                .columns
                .iter()
                .filter(|column| column.is_primary_key)
                .map(|column| column.name.clone())
                .collect::<Vec<_>>();

            let mut children = Vec::new();
            for relation in inbound {
                if let Some(child_table) = table_index.get(&(relation.child_schema.clone(), relation.child_table.clone())) {
                    children.push(AggregateChild {
                        schema: child_table.schema.clone(),
                        table: child_table.name.clone(),
                        foreign_key: relation.child_column.clone(),
                        parent_schema: relation.parent_schema.clone(),
                        parent_table: relation.parent_table.clone(),
                        parent_key: relation.parent_column.clone(),
                    });
                }
            }

            manifests.push(AggregateManifest {
                name: format!("{}.{}", table.schema, table.name),
                root: AggregateRoot {
                    schema: table.schema.clone(),
                    table: table.name.clone(),
                    primary_keys,
                },
                children,
            });
        }
    }

    manifests
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

fn build_inbound_relations(schema: &DatabaseSchema) -> BTreeMap<(String, String), BTreeSet<InboundRelation>> {
    let mut inbound = BTreeMap::new();

    for schema_info in &schema.schemas {
        for table in &schema_info.tables {
            for fk in &table.foreign_keys {
                inbound
                    .entry((fk.foreign_schema.clone(), fk.foreign_table.clone()))
                    .or_insert_with(BTreeSet::new)
                    .insert(InboundRelation {
                        child_schema: table.schema.clone(),
                        child_table: table.name.clone(),
                        child_column: fk.column.clone(),
                        parent_schema: fk.foreign_schema.clone(),
                        parent_table: fk.foreign_table.clone(),
                        parent_column: fk.foreign_column.clone(),
                    });
            }
        }
    }

    inbound
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct InboundRelation {
    child_schema: String,
    child_table: String,
    child_column: String,
    parent_schema: String,
    parent_table: String,
    parent_column: String,
}
