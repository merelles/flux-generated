use std::collections::{BTreeMap, BTreeSet};

use tokio_postgres::NoTls;

use crate::error::Result;
use crate::model::{ColumnInfo, DatabaseSchema, ForeignKeyInfo, SchemaInfo, TableInfo};

pub async fn reflect_database_from_url(database_url: &str) -> Result<DatabaseSchema> {
    let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;

    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("postgres connection error: {error}");
        }
    });

    reflect_database(&client).await
}

pub async fn reflect_database(client: &tokio_postgres::Client) -> Result<DatabaseSchema> {
    let mut schemas: BTreeMap<String, SchemaInfo> = BTreeMap::new();

    let column_rows = client
        .query(
            r#"
            SELECT
                c.table_schema,
                c.table_name,
                c.column_name,
                c.ordinal_position,
                c.is_nullable,
                c.data_type,
                c.udt_name,
                c.column_default
            FROM information_schema.columns c
            JOIN information_schema.tables t
              ON t.table_schema = c.table_schema
             AND t.table_name = c.table_name
            WHERE c.table_schema NOT IN ('information_schema', 'pg_catalog')
              AND t.table_type = 'BASE TABLE'
            ORDER BY c.table_schema, c.table_name, c.ordinal_position
            "#,
            &[],
        )
        .await?;

    let primary_key_rows = client
        .query(
            r#"
            SELECT
                n.nspname AS table_schema,
                c.relname AS table_name,
                a.attname AS column_name
            FROM pg_index i
            JOIN pg_class c ON c.oid = i.indrelid
            JOIN pg_namespace n ON n.oid = c.relnamespace
            JOIN pg_attribute a ON a.attrelid = c.oid
            WHERE i.indisprimary
              AND a.attnum = ANY(i.indkey)
              AND n.nspname NOT IN ('information_schema', 'pg_catalog')
            "#,
            &[],
        )
        .await?;

    let mut primary_keys: BTreeSet<(String, String, String)> = BTreeSet::new();
    for row in primary_key_rows {
        primary_keys.insert((row.get(0), row.get(1), row.get(2)));
    }

    let foreign_key_rows = client
        .query(
            r#"
            SELECT
                source_ns.nspname AS table_schema,
                source_tbl.relname AS table_name,
                source_col.attname AS column_name,
                fk.conname AS constraint_name,
                target_ns.nspname AS foreign_schema,
                target_tbl.relname AS foreign_table,
                target_col.attname AS foreign_column
            FROM pg_constraint fk
            JOIN pg_class source_tbl ON source_tbl.oid = fk.conrelid
            JOIN pg_namespace source_ns ON source_ns.oid = source_tbl.relnamespace
            JOIN pg_class target_tbl ON target_tbl.oid = fk.confrelid
            JOIN pg_namespace target_ns ON target_ns.oid = target_tbl.relnamespace
            JOIN unnest(fk.conkey) WITH ORDINALITY AS source_cols(attnum, ord) ON true
            JOIN unnest(fk.confkey) WITH ORDINALITY AS target_cols(attnum, ord) ON target_cols.ord = source_cols.ord
            JOIN pg_attribute source_col ON source_col.attrelid = source_tbl.oid AND source_col.attnum = source_cols.attnum
            JOIN pg_attribute target_col ON target_col.attrelid = target_tbl.oid AND target_col.attnum = target_cols.attnum
            WHERE fk.contype = 'f'
              AND source_ns.nspname NOT IN ('information_schema', 'pg_catalog')
            ORDER BY source_ns.nspname, source_tbl.relname, source_col.attname
            "#,
            &[],
        )
        .await?;

    let mut foreign_keys_by_table: BTreeMap<(String, String), BTreeMap<String, ForeignKeyInfo>> =
        BTreeMap::new();
    for row in foreign_key_rows {
        let schema: String = row.get("table_schema");
        let table: String = row.get("table_name");
        let foreign_key = ForeignKeyInfo {
            name: row.get("constraint_name"),
            column: row.get("column_name"),
            foreign_schema: row.get("foreign_schema"),
            foreign_table: row.get("foreign_table"),
            foreign_column: row.get("foreign_column"),
        };
        foreign_keys_by_table
            .entry((schema, table))
            .or_default()
            .entry(foreign_key.column.clone())
            .or_insert(foreign_key);
    }

    for row in column_rows {
        let schema_name: String = row.get("table_schema");
        let table_name: String = row.get("table_name");
        let column_name: String = row.get("column_name");
        let is_nullable: String = row.get("is_nullable");

        let schema = schemas.entry(schema_name.clone()).or_insert_with(|| SchemaInfo {
            name: schema_name.clone(),
            tables: Vec::new(),
        });

        let table_index = match schema
            .tables
            .iter()
            .position(|table| table.name == table_name)
        {
            Some(index) => index,
            None => {
                schema.tables.push(TableInfo {
                    schema: schema_name.clone(),
                    name: table_name.clone(),
                    columns: Vec::new(),
                    foreign_keys: foreign_keys_by_table
                        .get(&(schema_name.clone(), table_name.clone()))
                        .map(|relations| relations.values().cloned().collect())
                        .unwrap_or_default(),
                });
                schema.tables.len() - 1
            }
        };

        schema.tables[table_index].columns.push(ColumnInfo {
            name: column_name.clone(),
            ordinal_position: row.get("ordinal_position"),
            data_type: row.get("data_type"),
            udt_name: row.get("udt_name"),
            is_nullable: is_nullable == "YES",
            default_value: row.get("column_default"),
            is_primary_key: primary_keys.contains(&(
                schema_name.clone(),
                table_name.clone(),
                column_name,
            )),
        });
    }

    Ok(DatabaseSchema {
        schemas: schemas.into_values().collect(),
    })
}
