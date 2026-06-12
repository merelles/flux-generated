use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use generated_runtime::{FilterAst, PageResponse, PaginationInput, SearchCommand, SortDirection, SortItem};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use thiserror::Error;
use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Row};

#[derive(Clone)]
pub struct AppState {
    entities: Arc<HashMap<String, Arc<EntityService>>>,
    specs: Arc<Vec<EntitySpec>>,
}

impl AppState {
    pub async fn new(client: Arc<Client>) -> Result<Self, ApiError> {
        let specs = reflect_entity_specs(&client).await?;
        let entities = build_registry(&client, &specs);
        Ok(Self { entities: Arc::new(entities), specs: Arc::new(specs) })
    }

    fn service(&self, schema: &str, table: &str) -> Result<Arc<EntityService>, ApiError> {
        self.entities
            .get(&service_key(schema, table))
            .cloned()
            .ok_or_else(|| ApiError::NotFound { schema: schema.to_string(), table: table.to_string() })
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/docs", get(docs))
        .route("/swagger", get(swagger_ui))
        .route("/swagger-ui", get(swagger_ui))
        .route("/openapi.json", get(openapi_json))
        .route("/crud/:schema/:table", get(list_entities).post(create_entity))
        .route("/crud/:schema/:table/search", post(search_entities))
        .route("/crud/:schema/:table/:id", get(get_entity).put(update_entity).delete(delete_entity))
        .with_state(state)
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("entity not found: {schema}.{table}")]
    NotFound { schema: String, table: String },
    #[error("invalid payload: {0}")]
    InvalidPayload(String),
    #[error("invalid filter: {0}")]
    InvalidFilter(String),
    #[error("unsupported table without primary key: {schema}.{table}")]
    UnsupportedTable { schema: String, table: String },
    #[error("database error: {0}")]
    Database(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            ApiError::NotFound { .. } => StatusCode::NOT_FOUND,
            ApiError::InvalidPayload(_) | ApiError::InvalidFilter(_) | ApiError::UnsupportedTable { .. } => StatusCode::BAD_REQUEST,
            ApiError::Database(_) | ApiError::Serialization(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(json!({ "error": self.to_string() }))).into_response()
    }
}

#[derive(Debug, Clone)]
struct EntityService {
    client: Arc<Client>,
    spec: EntitySpec,
}

#[derive(Debug, Clone)]
struct EntitySpec {
    schema: String,
    table: String,
    tag: String,
    primary_key: Option<ColumnSpec>,
    columns: Vec<ColumnSpec>,
}

#[derive(Debug, Clone)]
struct ColumnSpec {
    name: String,
    udt_name: String,
    is_nullable: bool,
    is_primary_key: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct PaginationInputQuery {
    page: Option<u32>,
    per_page: Option<u32>,
}

impl PaginationInputQuery {
    fn into_pagination(self) -> PaginationInput {
        PaginationInput { page: self.page.unwrap_or(1), per_page: self.per_page.unwrap_or(25) }
    }
}

fn service_key(schema: &str, table: &str) -> String {
    format!("{}.{}", schema.trim().to_ascii_lowercase(), table.trim().to_ascii_lowercase())
}

fn quote_ident(value: &str) -> String { format!("\"{}\"", value.replace('"', "\"\"")) }
fn table_sql(spec: &EntitySpec) -> String { format!("{}.{}", quote_ident(&spec.schema), quote_ident(&spec.table)) }

fn entity_name(spec: &EntitySpec) -> String {
    spec.table.split('_').filter(|s| !s.is_empty()).map(|part| {
        let mut chars = part.chars();
        chars.next().map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default() + chars.as_str()
    }).collect()
}

fn column_schema(column: &ColumnSpec) -> Value {
    let mut schema = match column.udt_name.as_str() {
        "int2" | "int4" => json!({"type":"integer","format":"int32"}),
        "int8" => json!({"type":"integer","format":"int64"}),
        "float4" => json!({"type":"number","format":"float"}),
        "float8" => json!({"type":"number","format":"double"}),
        "bool" => json!({"type":"boolean"}),
        "json" | "jsonb" => json!({"type":"object","additionalProperties":true}),
        _ => json!({"type":"string"}),
    };
    if column.is_nullable {
        if let Value::Object(map) = &mut schema { map.insert("nullable".into(), Value::Bool(true)); }
    }
    schema
}

fn filter_ast_schema() -> Value {
    json!({
        "oneOf": [
            {"type":"object","required":["kind","filters"],"properties":{"kind":{"const":"and"},"filters":{"type":"array","items":{"$ref":"#/components/schemas/FilterAst"}}}},
            {"type":"object","required":["kind","filters"],"properties":{"kind":{"const":"or"},"filters":{"type":"array","items":{"$ref":"#/components/schemas/FilterAst"}}}},
            {"type":"object","required":["kind","filter"],"properties":{"kind":{"const":"not"},"filter":{"$ref":"#/components/schemas/FilterAst"}}},
            {"type":"object","required":["kind","field","value"],"properties":{"kind":{"const":"eq"},"field":{"type":"string"},"value":{}}},
            {"type":"object","required":["kind","field","value"],"properties":{"kind":{"const":"ne"},"field":{"type":"string"},"value":{}}},
            {"type":"object","required":["kind","field","value"],"properties":{"kind":{"const":"gt"},"field":{"type":"string"},"value":{}}},
            {"type":"object","required":["kind","field","value"],"properties":{"kind":{"const":"gte"},"field":{"type":"string"},"value":{}}},
            {"type":"object","required":["kind","field","value"],"properties":{"kind":{"const":"lt"},"field":{"type":"string"},"value":{}}},
            {"type":"object","required":["kind","field","value"],"properties":{"kind":{"const":"lte"},"field":{"type":"string"},"value":{}}},
            {"type":"object","required":["kind","field","value"],"properties":{"kind":{"const":"like"},"field":{"type":"string"},"value":{}}},
            {"type":"object","required":["kind","field","value"],"properties":{"kind":{"const":"ilike"},"field":{"type":"string"},"value":{}}},
            {"type":"object","required":["kind","field","values"],"properties":{"kind":{"const":"in"},"field":{"type":"string"},"values":{"type":"array","items":{}}}},
            {"type":"object","required":["kind","field"],"properties":{"kind":{"const":"is_null"},"field":{"type":"string"}}},
            {"type":"object","required":["kind","field"],"properties":{"kind":{"const":"is_not_null"},"field":{"type":"string"}}}
        ]
    })
}

fn pagination_schema() -> Value {
    json!({"type":"object","title":"PaginationInput","properties":{"page":{"type":"integer","format":"int32","minimum":1},"per_page":{"type":"integer","format":"int32","minimum":1}},"required":["page","per_page"]})
}

fn sort_direction_schema() -> Value { json!({"type":"string","enum":["Asc","Desc"]}) }
fn sort_item_schema() -> Value {
    json!({"type":"object","title":"SortItem","properties":{"field":{"type":"string"},"direction":{"$ref":"#/components/schemas/SortDirection"}},"required":["field","direction"]})
}

fn entity_response_schema(spec: &EntitySpec) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for column in &spec.columns {
        properties.insert(column.name.clone(), column_schema(column));
        if !column.is_nullable { required.push(column.name.clone()); }
    }
    json!({"type":"object","title":format!("{}Entity", entity_name(spec)),"properties":properties,"required":required})
}

fn create_command_schema(spec: &EntitySpec) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for column in &spec.columns {
        if column.is_primary_key { continue; }
        properties.insert(column.name.clone(), column_schema(column));
        if !column.is_nullable { required.push(column.name.clone()); }
    }
    json!({"type":"object","title":format!("{}CreateCommand", entity_name(spec)),"properties":properties,"required":required})
}

fn update_command_schema(spec: &EntitySpec) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    if let Some(pk) = &spec.primary_key {
        properties.insert(pk.name.clone(), column_schema(pk));
        required.push(pk.name.clone());
    }
    for column in &spec.columns {
        if column.is_primary_key { continue; }
        let mut schema = column_schema(column);
        if let Value::Object(map) = &mut schema { map.insert("nullable".into(), Value::Bool(true)); }
        properties.insert(column.name.clone(), schema);
    }
    json!({"type":"object","title":format!("{}UpdateCommand", entity_name(spec)),"properties":properties,"required":required})
}

fn search_command_schema(spec: &EntitySpec) -> Value {
    json!({"type":"object","title":format!("{}SearchCommand", entity_name(spec)),"properties":{"filter":{"$ref":"#/components/schemas/FilterAst"},"pagination":{"$ref":"#/components/schemas/PaginationInput"},"sort":{"type":"array","items":{"$ref":"#/components/schemas/SortItem"}}},"required":["pagination"]})
}

fn page_schema(spec: &EntitySpec) -> Value {
    json!({"type":"object","title":format!("{}Page", entity_name(spec)),"properties":{"page":{"type":"integer","format":"int32"},"per_page":{"type":"integer","format":"int32"},"total":{"type":"integer","format":"int64"},"total_pages":{"type":"integer","format":"int64"},"items":{"type":"array","items":{"$ref":format!("#/components/schemas/{}Entity", entity_name(spec))}}},"required":["page","per_page","total","total_pages","items"]})
}

fn build_registry(client: &Arc<Client>, specs: &[EntitySpec]) -> HashMap<String, Arc<EntityService>> {
    let mut registry = HashMap::new();
    for spec in specs {
        registry.insert(service_key(&spec.schema, &spec.table), Arc::new(EntityService { client: client.clone(), spec: spec.clone() }));
    }
    registry
}

async fn reflect_entity_specs(client: &Arc<Client>) -> Result<Vec<EntitySpec>, ApiError> {
    let columns = client.query(
        r#"
        SELECT c.table_schema, c.table_name, c.column_name, c.ordinal_position, c.is_nullable, c.udt_name
        FROM information_schema.columns c
        JOIN information_schema.tables t ON t.table_schema = c.table_schema AND t.table_name = c.table_name
        WHERE c.table_schema NOT IN ('information_schema', 'pg_catalog') AND t.table_type = 'BASE TABLE'
        ORDER BY c.table_schema, c.table_name, c.ordinal_position
        "#,
        &[],
    ).await.map_err(|err| ApiError::Database(err.to_string()))?;

    let primary_keys = client.query(
        r#"
        SELECT n.nspname AS table_schema, c.relname AS table_name, a.attname AS column_name
        FROM pg_index i
        JOIN pg_class c ON c.oid = i.indrelid
        JOIN pg_namespace n ON n.oid = c.relnamespace
        JOIN pg_attribute a ON a.attrelid = c.oid
        WHERE i.indisprimary AND a.attnum = ANY(i.indkey) AND n.nspname NOT IN ('information_schema', 'pg_catalog')
        "#,
        &[],
    ).await.map_err(|err| ApiError::Database(err.to_string()))?;

    let mut pk_index: HashMap<(String, String), String> = HashMap::new();
    for row in primary_keys { pk_index.insert((row.get(0), row.get(1)), row.get(2)); }

    let mut grouped: BTreeMap<(String, String), Vec<ColumnSpec>> = BTreeMap::new();
    for row in columns {
        let schema: String = row.get("table_schema");
        let table: String = row.get("table_name");
        let column_name: String = row.get("column_name");
        let is_nullable: String = row.get("is_nullable");
        let udt_name: String = row.get("udt_name");
        let is_primary_key = pk_index.get(&(schema.clone(), table.clone())).map(|pk: &String| pk == &column_name).unwrap_or(false);
        grouped.entry((schema, table)).or_default().push(ColumnSpec { name: column_name, udt_name, is_nullable: is_nullable == "YES", is_primary_key });
    }

    Ok(grouped.into_iter().map(|((schema, table), columns)| {
        let primary_key = columns.iter().find(|c| c.is_primary_key).cloned();
        EntitySpec { tag: format!("{}.{}", schema, table), schema, table, primary_key, columns }
    }).collect())
}

async fn openapi_json(State(state): State<AppState>) -> impl IntoResponse { Json(openapi_spec(&state)) }

fn openapi_spec(state: &AppState) -> Value {
    let mut paths = Map::new();
    let mut schemas = Map::new();
    let mut tags = Vec::new();
    schemas.insert("FilterAst".into(), filter_ast_schema());
    schemas.insert("PaginationInput".into(), pagination_schema());
    schemas.insert("SortDirection".into(), sort_direction_schema());
    schemas.insert("SortItem".into(), sort_item_schema());

    for spec in state.specs.iter() {
        tags.push(json!({"name": spec.tag, "description": format!("CRUD for {}.{}", spec.schema, spec.table)}));
        let entity = entity_name(spec);
        let entity_schema = format!("{}Entity", entity);
        let create_schema = format!("{}CreateCommand", entity);
        let update_schema = format!("{}UpdateCommand", entity);
        let search_schema = format!("{}SearchCommand", entity);
        let page_schema_name = format!("{}Page", entity);
        schemas.insert(entity_schema.clone(), entity_response_schema(spec));
        schemas.insert(create_schema.clone(), create_command_schema(spec));
        schemas.insert(update_schema.clone(), update_command_schema(spec));
        schemas.insert(search_schema.clone(), search_command_schema(spec));
        schemas.insert(page_schema_name.clone(), page_schema(spec));

        let base = format!("/crud/{}/{}", spec.schema, spec.table);
        let search = format!("{}/search", base);
        let item = format!("{}/{{id}}", base);

        paths.insert(base.clone(), json!({
            "get":{"tags":[spec.tag.clone()],"summary":format!("List {}", spec.tag),"responses":{"200":{"description":"Paged result","content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{}", page_schema_name)}}}}}},
            "post":{"tags":[spec.tag.clone()],"summary":format!("Create {}", spec.tag),"requestBody":{"required":true,"content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{}", create_schema)}}}},"responses":{"200":{"description":"Created entity","content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{}", entity_schema)}}}}}}
        }));
        paths.insert(search, json!({
            "post":{"tags":[spec.tag.clone()],"summary":format!("Search {}", spec.tag),"requestBody":{"required":true,"content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{}", search_schema)}}}},"responses":{"200":{"description":"Paged result","content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{}", page_schema_name)}}}}}}
        }));
        paths.insert(item, json!({
            "get":{"tags":[spec.tag.clone()],"summary":format!("Get {}", spec.tag),"responses":{"200":{"description":"Entity","content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{}", entity_schema)}}}}}},
            "put":{"tags":[spec.tag.clone()],"summary":format!("Update {}", spec.tag),"requestBody":{"required":true,"content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{}", update_schema)}}}},"responses":{"200":{"description":"Updated entity","content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{}", entity_schema)}}}}}},
            "delete":{"tags":[spec.tag.clone()],"summary":format!("Delete {}", spec.tag),"responses":{"200":{"description":"Deleted entity","content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{}", entity_schema)}}}}}}
        }));
    }

    json!({"openapi":"3.1.0","info":{"title":"flux-generated","version":"1.0.0"},"tags":tags,"paths":paths,"components":{"schemas":schemas}})
}

async fn list_entities(State(state): State<AppState>, Path((schema, table)): Path<(String, String)>, Query(query): Query<PaginationInputQuery>) -> Result<Json<Value>, ApiError> {
    let page = state.service(&schema, &table)?.list(SearchCommand { filter: None, pagination: query.into_pagination(), sort: Vec::new() }).await?;
    Ok(Json(serde_json::to_value(page).map_err(|err| ApiError::Serialization(err.to_string()))?))
}

async fn search_entities(State(state): State<AppState>, Path((schema, table)): Path<(String, String)>, Json(command): Json<SearchCommand>) -> Result<Json<Value>, ApiError> {
    let page = state.service(&schema, &table)?.search(command).await?;
    Ok(Json(serde_json::to_value(page).map_err(|err| ApiError::Serialization(err.to_string()))?))
}

async fn create_entity(State(state): State<AppState>, Path((schema, table)): Path<(String, String)>, Json(payload): Json<Value>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.service(&schema, &table)?.create(payload).await?))
}

async fn get_entity(State(state): State<AppState>, Path((schema, table, id)): Path<(String, String, String)>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.service(&schema, &table)?.get(Value::String(id)).await?))
}

async fn update_entity(State(state): State<AppState>, Path((schema, table, id)): Path<(String, String, String)>, Json(payload): Json<Value>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.service(&schema, &table)?.update(Value::String(id), payload).await?))
}

async fn delete_entity(State(state): State<AppState>, Path((schema, table, id)): Path<(String, String, String)>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.service(&schema, &table)?.delete(Value::String(id)).await?))
}

async fn health() -> impl IntoResponse { Json(json!({"status":"ok"})) }

async fn docs(State(state): State<AppState>) -> impl IntoResponse {
    let mut items = String::new();
    for spec in state.specs.iter() {
        items.push_str(&format!("<li><strong>{}</strong><br><code>GET /crud/{}/{}</code><br><code>POST /crud/{}/{}</code><br><code>POST /crud/{}/{}/search</code><br><code>GET/PUT/DELETE /crud/{}/{}/{{id}}</code></li>", spec.tag, spec.schema, spec.table, spec.schema, spec.table, spec.schema, spec.table, spec.schema, spec.table));
    }
    Html(format!(r#"<!doctype html><html lang="pt-BR"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>CRUD Docs</title></head><body><h1>Documentacao CRUD</h1><p><a href="/swagger-ui">Abrir Swagger UI</a></p><ul>{items}</ul></body></html>"#))
}

async fn swagger_ui() -> impl IntoResponse {
    Html(r#"<!doctype html><html lang="pt-BR"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Swagger UI</title><link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css"></head><body><div id="swagger-ui"></div><script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script><script>window.ui = SwaggerUIBundle({ url: '/openapi.json', dom_id: '#swagger-ui' });</script></body></html>"#)
}

impl EntityService {
    async fn create(&self, payload: Value) -> Result<Value, ApiError> {
        let payload = payload.as_object().cloned().ok_or_else(|| ApiError::InvalidPayload("expected object".into()))?;
        let columns = payload_columns(&self.spec, &payload, false)?;
        let sql = format!("WITH inserted AS (INSERT INTO {} ({}) VALUES ({}) RETURNING *) SELECT row_to_json(inserted)::text AS payload FROM inserted", table_sql(&self.spec), columns.names.join(", "), columns.placeholders.join(", "));
        self.query_one_json(sql, columns.params).await
    }

    async fn update(&self, id: Value, payload: Value) -> Result<Value, ApiError> {
        let payload = payload.as_object().cloned().ok_or_else(|| ApiError::InvalidPayload("expected object".into()))?;
        let columns = payload_columns(&self.spec, &payload, true)?;
        if columns.names.is_empty() { return Err(ApiError::InvalidPayload("update payload has no mutable fields".into())); }
        let pk = self.spec.primary_key.as_ref().ok_or_else(|| ApiError::UnsupportedTable { schema: self.spec.schema.clone(), table: self.spec.table.clone() })?;
        let mut params = columns.params;
        params.push(json_to_param(&id, &pk.udt_name)?);
        let sets = columns.names.iter().enumerate().map(|(idx, name)| format!("{} = ${}", name, idx + 1)).collect::<Vec<_>>();
        let sql = format!("WITH updated AS (UPDATE {} SET {} WHERE {} = ${} RETURNING *) SELECT row_to_json(updated)::text AS payload FROM updated", table_sql(&self.spec), sets.join(", "), quote_ident(&pk.name), params.len());
        self.query_one_json(sql, params).await
    }

    async fn delete(&self, id: Value) -> Result<Value, ApiError> {
        let pk = self.spec.primary_key.as_ref().ok_or_else(|| ApiError::UnsupportedTable { schema: self.spec.schema.clone(), table: self.spec.table.clone() })?;
        let sql = format!("WITH deleted AS (DELETE FROM {} WHERE {} = $1 RETURNING *) SELECT row_to_json(deleted)::text AS payload FROM deleted", table_sql(&self.spec), quote_ident(&pk.name));
        self.query_one_json(sql, vec![json_to_param(&id, &pk.udt_name)?]).await
    }

    async fn get(&self, id: Value) -> Result<Value, ApiError> {
        let pk = self.spec.primary_key.as_ref().ok_or_else(|| ApiError::UnsupportedTable { schema: self.spec.schema.clone(), table: self.spec.table.clone() })?;
        let sql = format!("SELECT row_to_json(t)::text AS payload FROM (SELECT * FROM {} WHERE {} = $1) t", table_sql(&self.spec), quote_ident(&pk.name));
        self.query_one_json(sql, vec![json_to_param(&id, &pk.udt_name)?]).await
    }

    async fn list(&self, command: SearchCommand) -> Result<PageResponse, ApiError> { self.search(command).await }

    async fn search(&self, command: SearchCommand) -> Result<PageResponse, ApiError> {
        let mut params = Vec::<Box<dyn ToSql + Send + Sync>>::new();
        let where_clause = match &command.filter {
            Some(filter) => {
                let clause = build_filter_sql(&self.spec, filter, &mut params)?;
                if clause.is_empty() { String::new() } else { format!("WHERE {clause}") }
            }
            None => String::new(),
        };
        let order_clause = build_order_clause(&self.spec, &command.sort)?;
        let page = command.pagination.page.max(1);
        let per_page = command.pagination.per_page.max(1).min(100);
        let offset = ((page - 1) * per_page) as i64;

        let total_sql = format!("SELECT COUNT(*)::bigint AS total FROM {} {}", table_sql(&self.spec), where_clause);
        let total = {
            let refs = param_refs(&params);
            let row = self.client.query_one(&total_sql, &refs).await.map_err(|err| ApiError::Database(err.to_string()))?;
            row.get::<_, i64>("total")
        };

        let mut page_params = params;
        page_params.push(Box::new(per_page as i64));
        page_params.push(Box::new(offset));
        let list_sql = format!("SELECT row_to_json(t)::text AS payload FROM (SELECT * FROM {} {} {} LIMIT ${} OFFSET ${}) t", table_sql(&self.spec), where_clause, order_clause, page_params.len() - 1, page_params.len());
        let rows = {
            let refs = param_refs(&page_params);
            self.client.query(&list_sql, &refs).await.map_err(|err| ApiError::Database(err.to_string()))?
        };
        let items = rows.into_iter().map(|row| parse_row_json(&row)).collect::<Result<Vec<_>, _>>()?;
        let total_pages = if total == 0 { 0 } else { ((total as f64) / (per_page as f64)).ceil() as i64 };
        Ok(PageResponse { page, per_page, total, total_pages, items })
    }

    async fn query_one_json(&self, sql: String, params: Vec<Box<dyn ToSql + Send + Sync>>) -> Result<Value, ApiError> {
        let refs = param_refs(&params);
        let row = self.client.query_one(&sql, &refs).await.map_err(|err| ApiError::Database(err.to_string()))?;
        parse_row_json(&row)
    }
}

struct PayloadColumns {
    names: Vec<String>,
    placeholders: Vec<String>,
    params: Vec<Box<dyn ToSql + Send + Sync>>,
}

fn payload_columns(spec: &EntitySpec, payload: &Map<String, Value>, ignore_primary_key: bool) -> Result<PayloadColumns, ApiError> {
    let mut names = Vec::new();
    let mut placeholders = Vec::new();
    let mut params = Vec::new();
    for column in &spec.columns {
        if ignore_primary_key && column.is_primary_key { continue; }
        if let Some(value) = payload.get(&column.name) {
            names.push(quote_ident(&column.name));
            placeholders.push(format!("${}", placeholders.len() + 1));
            params.push(json_to_param(value, &column.udt_name)?);
        }
    }
    Ok(PayloadColumns { names, placeholders, params })
}

fn build_filter_sql(spec: &EntitySpec, filter: &FilterAst, params: &mut Vec<Box<dyn ToSql + Send + Sync>>) -> Result<String, ApiError> {
    match filter {
        FilterAst::And { filters } => Ok(format!("({})", filters.iter().map(|child| build_filter_sql(spec, child, params)).collect::<Result<Vec<_>, _>>()?.join(" AND "))),
        FilterAst::Or { filters } => Ok(format!("({})", filters.iter().map(|child| build_filter_sql(spec, child, params)).collect::<Result<Vec<_>, _>>()?.join(" OR "))),
        FilterAst::Not { filter } => Ok(format!("NOT ({})", build_filter_sql(spec, filter, params)?)),
        FilterAst::Eq { field, value } => comparison_sql(spec, field, value, "=", params),
        FilterAst::Ne { field, value } => comparison_sql(spec, field, value, "<>", params),
        FilterAst::Gt { field, value } => comparison_sql(spec, field, value, ">", params),
        FilterAst::Gte { field, value } => comparison_sql(spec, field, value, ">=", params),
        FilterAst::Lt { field, value } => comparison_sql(spec, field, value, "<", params),
        FilterAst::Lte { field, value } => comparison_sql(spec, field, value, "<=", params),
        FilterAst::Like { field, value } => comparison_sql(spec, field, value, "LIKE", params),
        FilterAst::ILike { field, value } => comparison_sql(spec, field, value, "ILIKE", params),
        FilterAst::In { field, values } => {
            let column = column_by_name(spec, field)?;
            let mut placeholders = Vec::new();
            for value in values {
                params.push(json_to_param(value, &column.udt_name)?);
                placeholders.push(format!("${}", params.len()));
            }
            Ok(format!("{} IN ({})", quote_ident(field), placeholders.join(", ")))
        }
        FilterAst::IsNull { field } => { column_by_name(spec, field)?; Ok(format!("{} IS NULL", quote_ident(field))) }
        FilterAst::IsNotNull { field } => { column_by_name(spec, field)?; Ok(format!("{} IS NOT NULL", quote_ident(field))) }
    }
}

fn comparison_sql(spec: &EntitySpec, field: &str, value: &Value, operator: &str, params: &mut Vec<Box<dyn ToSql + Send + Sync>>) -> Result<String, ApiError> {
    let column = column_by_name(spec, field)?;
    params.push(json_to_param(value, &column.udt_name)?);
    Ok(format!("{} {} ${}", quote_ident(field), operator, params.len()))
}

fn build_order_clause(spec: &EntitySpec, sort: &[SortItem]) -> Result<String, ApiError> {
    if sort.is_empty() {
        return Ok(spec.primary_key.as_ref().map(|pk| format!("ORDER BY {}", quote_ident(&pk.name))).unwrap_or_default());
    }
    let mut clauses = Vec::new();
    for item in sort {
        column_by_name(spec, &item.field)?;
        let direction = match item.direction { SortDirection::Asc => "ASC", SortDirection::Desc => "DESC" };
        clauses.push(format!("{} {}", quote_ident(&item.field), direction));
    }
    Ok(format!("ORDER BY {}", clauses.join(", ")))
}

fn column_by_name<'a>(spec: &'a EntitySpec, field: &str) -> Result<&'a ColumnSpec, ApiError> {
    spec.columns.iter().find(|column| column.name.eq_ignore_ascii_case(field)).ok_or_else(|| ApiError::InvalidFilter(format!("unknown field {field}")))
}

fn parse_row_json(row: &Row) -> Result<Value, ApiError> {
    let payload: String = row.get("payload");
    serde_json::from_str(&payload).map_err(|err| ApiError::Serialization(err.to_string()))
}

fn json_to_param(value: &Value, udt_name: &str) -> Result<Box<dyn ToSql + Send + Sync>, ApiError> {
    if value.is_null() { return Ok(Box::new(Option::<String>::None)); }
    match udt_name {
        "int2" => Ok(Box::new(value.as_i64().unwrap_or_default() as i16)),
        "int4" => Ok(Box::new(value.as_i64().unwrap_or_default() as i32)),
        "int8" => Ok(Box::new(value.as_i64().unwrap_or_default())),
        "float4" => Ok(Box::new(value.as_f64().unwrap_or_default() as f32)),
        "float8" => Ok(Box::new(value.as_f64().unwrap_or_default())),
        "bool" => Ok(Box::new(value.as_bool().unwrap_or_default())),
        "json" | "jsonb" => Ok(Box::new(value.to_string())),
        _ => Ok(Box::new(value.as_str().map(|s| s.to_string()).unwrap_or_else(|| value.to_string()))),
    }
}

fn param_refs<'a>(params: &'a [Box<dyn ToSql + Send + Sync>]) -> Vec<&'a (dyn ToSql + Sync)> {
    params.iter().map(|param| param.as_ref() as &(dyn ToSql + Sync)).collect()
}
