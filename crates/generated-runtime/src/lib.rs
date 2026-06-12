use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FilterAst {
    And { filters: Vec<FilterAst> },
    Or { filters: Vec<FilterAst> },
    Not { filter: Box<FilterAst> },
    Eq { field: String, value: Value },
    Ne { field: String, value: Value },
    Gt { field: String, value: Value },
    Gte { field: String, value: Value },
    Lt { field: String, value: Value },
    Lte { field: String, value: Value },
    Like { field: String, value: Value },
    ILike { field: String, value: Value },
    In { field: String, values: Vec<Value> },
    IsNull { field: String },
    IsNotNull { field: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationInput {
    pub page: u32,
    pub per_page: u32,
}

impl Default for PaginationInput {
    fn default() -> Self {
        Self {
            page: 1,
            per_page: 25,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortItem {
    pub field: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchCommand {
    pub filter: Option<FilterAst>,
    pub pagination: PaginationInput,
    pub sort: Vec<SortItem>,
}

impl Default for SearchCommand {
    fn default() -> Self {
        Self {
            filter: None,
            pagination: PaginationInput::default(),
            sort: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PageResponse {
    pub page: u32,
    pub per_page: u32,
    pub total: i64,
    pub total_pages: i64,
    pub items: Vec<Value>,
}
