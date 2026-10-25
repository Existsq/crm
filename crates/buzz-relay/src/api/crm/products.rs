//! `GET /crm/products` and `GET /crm/products/{id}` — the product catalog.

use std::sync::Arc;

use axum::{
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use buzz_db::crm::{ProductQuery, MAX_PRODUCT_PAGE};

use super::authorize_space_member;
use crate::{
    api::{api_error, internal_error, not_found, parse_query_or_400},
    state::AppState,
};

/// Route path of the product list.
pub const PRODUCTS_PATH: &str = "/crm/products";
/// Route path of one product.
pub const PRODUCT_PATH: &str = "/crm/products/{product_id}";

const DEFAULT_PRODUCT_LIMIT: i64 = 50;
/// Longest accepted search text, in characters.
const MAX_QUERY_CHARS: usize = 128;

/// Query parameters of `GET /crm/products`.
#[derive(Debug, Deserialize, Default)]
pub struct ProductsParams {
    q: Option<String>,
    barcode: Option<String>,
    archived: Option<bool>,
    /// `services` or `goods`.
    kind: Option<String>,
    limit: Option<i64>,
    after: Option<Uuid>,
}

fn product_query(params: ProductsParams) -> Result<ProductQuery, &'static str> {
    let limit = params.limit.unwrap_or(DEFAULT_PRODUCT_LIMIT);
    if !(1..=MAX_PRODUCT_PAGE).contains(&limit) {
        return Err("limit must be between 1 and 200");
    }
    let too_long = |value: &Option<String>| {
        value
            .as_deref()
            .is_some_and(|v| v.chars().count() > MAX_QUERY_CHARS)
    };
    if too_long(&params.q) || too_long(&params.barcode) {
        return Err("q and barcode must be at most 128 characters");
    }
    let services = match params.kind.as_deref() {
        None => None,
        Some("services") => Some(true),
        Some("goods") => Some(false),
        Some(_) => return Err("kind must be services or goods"),
    };
    Ok(ProductQuery {
        q: params.q,
        barcode: params.barcode,
        include_archived: params.archived.unwrap_or(false),
        services,
        after: params.after,
        limit,
    })
}

/// `GET /crm/products` — one keyset page of the space's products.
///
/// `q` matches a name substring or SKU prefix (case-insensitive), `barcode`
/// an exact barcode, `archived=true` includes archived products, `after` is
/// the `next` cursor of the previous page.
pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, PRODUCTS_PATH, raw_query.as_deref()).await {
            Ok(member) => member,
            Err(response) => return response,
        };
    // Parse after admission so malformed parameters cannot pre-empt the
    // authentication contract (same order as the workflow reads).
    let params: ProductsParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    let query = match product_query(params) {
        Ok(query) => query,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, message).into_response(),
    };
    match state
        .db
        .crm_list_products(member.community(), member.space.channel_id, &query)
        .await
    {
        Ok(page) => Json(json!({ "products": page.products, "next": page.next })).into_response(),
        Err(e) => internal_error(&format!("CRM products read: {e}")).into_response(),
    }
}

/// `GET /crm/products/{id}` — one product, archived or not.
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(product_id): Path<Uuid>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let path = format!("{PRODUCTS_PATH}/{product_id}");
    let member = match authorize_space_member(&state, &headers, &path, raw_query.as_deref()).await {
        Ok(member) => member,
        Err(response) => return response,
    };
    match state
        .db
        .crm_get_product(member.community(), member.space.channel_id, product_id)
        .await
    {
        Ok(Some(product)) => Json::<Value>(json!(product)).into_response(),
        Ok(None) => not_found("product not found").into_response(),
        Err(e) => internal_error(&format!("CRM product read: {e}")).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_defaults_and_is_bounded() {
        let query = product_query(ProductsParams::default()).unwrap();
        assert_eq!(query.limit, DEFAULT_PRODUCT_LIMIT);
        assert!(!query.include_archived);
        for bad in [0, -1, MAX_PRODUCT_PAGE + 1] {
            let params = ProductsParams {
                limit: Some(bad),
                ..ProductsParams::default()
            };
            assert!(product_query(params).is_err(), "limit {bad}");
        }
        let params = ProductsParams {
            limit: Some(MAX_PRODUCT_PAGE),
            archived: Some(true),
            ..ProductsParams::default()
        };
        let query = product_query(params).unwrap();
        assert_eq!(query.limit, MAX_PRODUCT_PAGE);
        assert!(query.include_archived);
    }

    #[test]
    fn oversized_search_text_is_rejected() {
        let params = ProductsParams {
            q: Some("я".repeat(MAX_QUERY_CHARS + 1)),
            ..ProductsParams::default()
        };
        assert!(product_query(params).is_err());
        let params = ProductsParams {
            barcode: Some("1".repeat(MAX_QUERY_CHARS)),
            ..ProductsParams::default()
        };
        assert!(product_query(params).is_ok());
    }

    #[test]
    fn product_json_uses_wire_names() {
        let record = buzz_db::crm::CrmProductRecord {
            product_id: Uuid::nil(),
            name: "Чай".into(),
            sku: None,
            barcodes: vec!["INT-1".into()],
            unit: "piece".into(),
            price_minor: 100,
            vat: "vat22".into(),
            payment_object: "commodity".into(),
            category: None,
            min_stock_milli: 0,
            requires_marking: false,
            duration_min: None,
            archived: false,
            revision_event_id: vec![0xab; 32],
            updated_at: chrono::Utc::now(),
        };
        let value = json!(record);
        assert_eq!(value["id"], json!(Uuid::nil()));
        assert_eq!(value["revision"], json!("ab".repeat(32)));
        assert!(value.get("product_id").is_none());
        assert!(value.get("revision_event_id").is_none());
    }
}
