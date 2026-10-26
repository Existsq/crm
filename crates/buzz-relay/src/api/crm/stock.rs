//! `GET /crm/stock`, `/crm/stock/moves`, `/crm/stock/docs[/{id}]` and
//! `/crm/tech-cards` — the stock ledger and the consumables of services.

use std::sync::Arc;

use axum::{
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use buzz_crm::StockDocType;
use buzz_db::crm::{StockDocQuery, StockMoveQuery, StockQuery, MAX_STOCK_PAGE};

use super::authorize_space_member;
use crate::{
    api::{api_error, internal_error, not_found, parse_query_or_400},
    state::AppState,
};

/// Route path of the stock levels.
pub const STOCK_PATH: &str = "/crm/stock";
/// Route path of the movement journal.
pub const MOVES_PATH: &str = "/crm/stock/moves";
/// Route path of the stock documents.
pub const DOCS_PATH: &str = "/crm/stock/docs";
/// Route path of one stock document.
pub const DOC_PATH: &str = "/crm/stock/docs/{doc_id}";
/// Route path of the tech cards.
pub const TECH_CARDS_PATH: &str = "/crm/tech-cards";

const DEFAULT_PAGE: i64 = 100;
const MAX_SEARCH_CHARS: usize = 64;

/// Query parameters of `GET /crm/stock`.
#[derive(Debug, Deserialize, Default)]
pub struct StockParams {
    warehouse: Option<Uuid>,
    q: Option<String>,
    low: Option<bool>,
}

/// Query parameters of `GET /crm/stock/moves`.
#[derive(Debug, Deserialize, Default)]
pub struct MovesParams {
    warehouse: Option<Uuid>,
    product: Option<Uuid>,
    before: Option<i64>,
    limit: Option<i64>,
}

/// Query parameters of `GET /crm/stock/docs`.
#[derive(Debug, Deserialize, Default)]
pub struct DocsParams {
    #[serde(rename = "type")]
    doc_type: Option<StockDocType>,
    before: Option<i64>,
    limit: Option<i64>,
}

fn page_limit(limit: Option<i64>) -> Result<i64, &'static str> {
    let limit = limit.unwrap_or(DEFAULT_PAGE);
    if (1..=MAX_STOCK_PAGE).contains(&limit) {
        Ok(limit)
    } else {
        Err("limit must be between 1 and 500")
    }
}

fn stock_query(params: StockParams) -> Result<StockQuery, &'static str> {
    let search = params
        .q
        .map(|q| q.trim().to_owned())
        .filter(|q| !q.is_empty());
    if search
        .as_deref()
        .is_some_and(|q| q.chars().count() > MAX_SEARCH_CHARS)
    {
        return Err("q must be at most 64 characters");
    }
    Ok(StockQuery {
        warehouse_id: params.warehouse,
        search,
        low_only: params.low.unwrap_or(false),
    })
}

fn moves_query(params: MovesParams) -> Result<StockMoveQuery, &'static str> {
    if params.before.is_some_and(|before| before < 1) {
        return Err("before must be a move id");
    }
    Ok(StockMoveQuery {
        warehouse_id: params.warehouse,
        product_id: params.product,
        before: params.before,
        limit: page_limit(params.limit)?,
    })
}

fn docs_query(params: DocsParams) -> Result<StockDocQuery, &'static str> {
    if params.before.is_some_and(|before| before < 1) {
        return Err("before must be a document number");
    }
    Ok(StockDocQuery {
        doc_type: params.doc_type.map(|t| t.as_str().to_owned()),
        before: params.before,
        limit: page_limit(params.limit)?,
    })
}

macro_rules! member_or_return {
    ($state:expr, $headers:expr, $path:expr, $raw:expr) => {
        match authorize_space_member(&$state, &$headers, $path, $raw.as_deref()).await {
            Ok(member) => member,
            Err(response) => return response,
        }
    };
}

macro_rules! params_or_return {
    ($raw:expr, $build:expr) => {{
        let params = match parse_query_or_400($raw.as_deref()) {
            Ok(params) => params,
            Err(response) => return response.into_response(),
        };
        match $build(params) {
            Ok(query) => query,
            Err(message) => return api_error(StatusCode::BAD_REQUEST, message).into_response(),
        }
    }};
}

/// `GET /crm/stock` — quantity and cost value of every stocked product on
/// one `warehouse` (all warehouses when absent), with totals. `q` searches
/// names and SKUs; `low=true` keeps products at or below their minimum.
pub async fn levels(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member = member_or_return!(state, headers, STOCK_PATH, raw_query);
    let query = params_or_return!(raw_query, stock_query);
    match state
        .db
        .crm_list_stock(member.community(), member.space.channel_id, &query)
        .await
    {
        Ok(list) => Json(json!({
            "warehouse_id": query.warehouse_id,
            "truncated": list.matched > list.items.len() as i64,
            "items": list.items,
            "totals": { "value_minor": list.value_minor, "low_count": list.low_count },
        }))
        .into_response(),
        Err(e) => internal_error(&format!("CRM stock read: {e}")).into_response(),
    }
}

/// `GET /crm/stock/moves` — the movement journal, newest first; `before` is
/// the `next` cursor.
pub async fn moves(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member = member_or_return!(state, headers, MOVES_PATH, raw_query);
    let query = params_or_return!(raw_query, moves_query);
    match state
        .db
        .crm_list_stock_moves(member.community(), member.space.channel_id, &query)
        .await
    {
        Ok(page) => Json(json!({ "moves": page.moves, "next": page.next })).into_response(),
        Err(e) => internal_error(&format!("CRM stock moves read: {e}")).into_response(),
    }
}

/// `GET /crm/stock/docs` — stock documents, newest first; `type` narrows to
/// one kind, `before` is the `next` cursor (a document number).
pub async fn docs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member = member_or_return!(state, headers, DOCS_PATH, raw_query);
    let query = params_or_return!(raw_query, docs_query);
    match state
        .db
        .crm_list_stock_docs(member.community(), member.space.channel_id, &query)
        .await
    {
        Ok(page) => Json(json!({ "docs": page.docs, "next": page.next })).into_response(),
        Err(e) => internal_error(&format!("CRM stock documents read: {e}")).into_response(),
    }
}

/// `GET /crm/stock/docs/{id}` — one stock document with its lines.
pub async fn doc(
    State(state): State<Arc<AppState>>,
    Path(doc_id): Path<Uuid>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let path = format!("{DOCS_PATH}/{doc_id}");
    let member = member_or_return!(state, headers, &path, raw_query);
    match state
        .db
        .crm_get_stock_doc(member.community(), member.space.channel_id, doc_id)
        .await
    {
        Ok(Some(doc)) => Json::<Value>(json!(doc)).into_response(),
        Ok(None) => not_found("stock document not found").into_response(),
        Err(e) => internal_error(&format!("CRM stock document read: {e}")).into_response(),
    }
}

/// `GET /crm/tech-cards` — the consumables of every service that has them.
pub async fn tech_cards(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member = member_or_return!(state, headers, TECH_CARDS_PATH, raw_query);
    match state
        .db
        .crm_list_tech_cards(member.community(), member.space.channel_id)
        .await
    {
        Ok(cards) => Json(json!({ "cards": cards })).into_response(),
        Err(e) => internal_error(&format!("CRM tech cards read: {e}")).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_limits_are_bounded() {
        assert_eq!(page_limit(None), Ok(DEFAULT_PAGE));
        assert!(page_limit(Some(0)).is_err());
        assert!(page_limit(Some(MAX_STOCK_PAGE + 1)).is_err());
        assert!(moves_query(MovesParams {
            before: Some(0),
            ..MovesParams::default()
        })
        .is_err());
        assert!(docs_query(DocsParams {
            before: Some(0),
            ..DocsParams::default()
        })
        .is_err());
    }

    #[test]
    fn stock_search_is_trimmed_and_bounded() {
        let query = |q: &str| {
            stock_query(StockParams {
                q: Some(q.into()),
                ..StockParams::default()
            })
        };
        assert_eq!(query("  ").unwrap().search, None);
        assert_eq!(query(" шамп ").unwrap().search.as_deref(), Some("шамп"));
        assert!(query(&"я".repeat(65)).is_err());
    }

    #[test]
    fn doc_type_filter_uses_wire_names() {
        let query = docs_query(DocsParams {
            doc_type: Some(StockDocType::WriteOff),
            ..DocsParams::default()
        })
        .unwrap();
        assert_eq!(query.doc_type.as_deref(), Some("write_off"));
    }
}
