//! CRM catalog commands (relay kind 42102, `GET /crm/products`).
//!
//! Reads return the relay's read model unchanged; writes build the product
//! event with `buzz_sdk::build_crm_product` (which runs the relay's payload
//! validation), sign it with the active identity and submit it over HTTP.

use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use buzz_crm_pkg::{Money, PaymentObject, ProductPayload, Qty, Unit, VatRate};

use super::crm::{parse_uuid, sdk_error, CrmWriteWire};
use crate::{
    app_state::AppState,
    relay::{get_relay_json, submit_event},
};

/// Relay page-size bound for `GET /crm/products`.
const MAX_PRODUCTS_PAGE: u32 = 200;

/// Product write input. Mirrors `CrmProductWriteInput` in `tauriCrmProducts.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmProductWriteInput {
    pub channel_id: String,
    /// Product id. For a create the client picks it once per dialog, so a
    /// retry after a lost response cannot create a second product.
    pub product_id: String,
    /// Current revision for an update; `None` creates the product.
    pub expected_revision: Option<String>,
    pub name: String,
    pub sku: Option<String>,
    #[serde(default)]
    pub barcodes: Vec<String>,
    pub unit: Unit,
    pub price_minor: Money,
    pub vat: VatRate,
    pub payment_object: PaymentObject,
    pub category: Option<String>,
    #[serde(default)]
    pub min_stock_milli: Qty,
    #[serde(default)]
    pub requires_marking: bool,
    #[serde(default)]
    pub archived: bool,
    /// Booking duration of a service, in minutes.
    #[serde(default)]
    pub duration_min: Option<u32>,
}

/// `GET /crm/products` as returned by the relay (`{products, next}`).
#[tauri::command]
pub async fn crm_list_products(
    query: Option<String>,
    barcode: Option<String>,
    archived: Option<bool>,
    limit: Option<u32>,
    after: Option<String>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let path = products_path(
        query.as_deref(),
        barcode.as_deref(),
        archived.unwrap_or(false),
        limit,
        after.as_deref(),
    );
    get_relay_json::<Value>(&state, &path).await
}

/// `GET /crm/products/{id}` as returned by the relay.
#[tauri::command]
pub async fn crm_get_product(
    product_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let id = parse_uuid(&product_id, "product id")?;
    get_relay_json::<Value>(&state, &format!("/crm/products/{id}")).await
}

/// Create or update a product.
#[tauri::command]
pub async fn crm_write_product(
    input: CrmProductWriteInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let (builder, product_id) = product_builder(input)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(product_id.to_string()),
    })
}

/// `/crm/products` with the non-empty filters percent-encoded.
fn products_path(
    query: Option<&str>,
    barcode: Option<&str>,
    archived: bool,
    limit: Option<u32>,
    after: Option<&str>,
) -> String {
    fn non_blank(value: Option<&str>) -> Option<&str> {
        value.map(str::trim).filter(|v| !v.is_empty())
    }
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    if let Some(q) = non_blank(query) {
        params.append_pair("q", q);
    }
    if let Some(code) = non_blank(barcode) {
        params.append_pair("barcode", code);
    }
    if archived {
        params.append_pair("archived", "true");
    }
    if let Some(limit) = limit {
        params.append_pair("limit", &limit.clamp(1, MAX_PRODUCTS_PAGE).to_string());
    }
    if let Some(cursor) = after.filter(|c| !c.is_empty()) {
        params.append_pair("after", cursor);
    }
    let encoded = params.finish();
    if encoded.is_empty() {
        "/crm/products".to_string()
    } else {
        format!("/crm/products?{encoded}")
    }
}

fn product_builder(
    input: CrmProductWriteInput,
) -> Result<(nostr::EventBuilder, uuid::Uuid), String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let product_id = parse_uuid(&input.product_id, "product id")?;
    let expected_revision = input.expected_revision.as_deref();
    let payload = ProductPayload {
        v: 1,
        name: input.name,
        sku: input.sku,
        barcodes: input.barcodes,
        unit: input.unit,
        price_minor: input.price_minor,
        vat: input.vat,
        payment_object: input.payment_object,
        category: input.category,
        min_stock_milli: input.min_stock_milli,
        requires_marking: input.requires_marking,
        archived: input.archived,
        duration_min: input.duration_min,
    };
    let builder = buzz_sdk_pkg::build_crm_product(channel, product_id, payload, expected_revision)
        .map_err(sdk_error)?;
    Ok((builder, product_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(product_id: &str, revision: Option<&str>) -> CrmProductWriteInput {
        serde_json::from_value(json!({
            "channel_id": uuid::Uuid::new_v4().to_string(),
            "product_id": product_id,
            "expected_revision": revision,
            "name": " Молоко 3,2% ",
            "sku": " MLK-001 ",
            "barcodes": ["4600051000057"],
            "unit": "piece",
            "price_minor": 8990,
            "vat": "vat10",
            "payment_object": "commodity",
            "category": null,
            "min_stock_milli": 5000,
        }))
        .expect("input deserializes")
    }

    fn tag(event: &nostr::Event, name: &str) -> Option<String> {
        event
            .tags
            .iter()
            .find(|t| t.as_slice().first().map(String::as_str) == Some(name))
            .and_then(|t| t.as_slice().get(1).cloned())
    }

    #[test]
    fn create_uses_the_client_id_and_normalized_content() {
        let id = uuid::Uuid::new_v4().to_string();
        let (builder, built_id) = product_builder(input(&id, None)).unwrap();
        let event = builder.sign_with_keys(&nostr::Keys::generate()).unwrap();
        assert_eq!(event.kind.as_u16(), 42102);
        assert_eq!(built_id.to_string(), id);
        assert_eq!(tag(&event, "crm-id"), Some(id));
        assert_eq!(tag(&event, "expected-revision"), None);
        let content: Value = serde_json::from_str(&event.content).unwrap();
        assert_eq!(content["v"], 1);
        assert_eq!(content["name"], "Молоко 3,2%");
        assert_eq!(content["sku"], "MLK-001");
        assert_eq!(content["price_minor"], 8990);
        assert_eq!(content["min_stock_milli"], 5000);
        assert_eq!(content["archived"], false);
    }

    #[test]
    fn update_keeps_id_and_carries_revision() {
        let id = uuid::Uuid::new_v4().to_string();
        let revision = "ab".repeat(32);
        let (builder, built_id) = product_builder(input(&id, Some(&revision))).unwrap();
        let event = builder.sign_with_keys(&nostr::Keys::generate()).unwrap();
        assert_eq!(built_id.to_string(), id);
        assert_eq!(tag(&event, "crm-id"), Some(id));
        assert_eq!(tag(&event, "expected-revision"), Some(revision));
    }

    #[test]
    fn malformed_ids_and_revisions_are_rejected() {
        let id = uuid::Uuid::new_v4().to_string();
        assert!(product_builder(input("not-a-uuid", None)).is_err());
        assert!(product_builder(input(&id, Some("not-hex"))).is_err());
    }

    #[test]
    fn invalid_payloads_surface_as_invalid_errors() {
        let id = uuid::Uuid::new_v4().to_string();
        let mut fractional_piece = input(&id, None);
        fractional_piece.min_stock_milli = Qty::from_milli(1500).unwrap();
        let mut historical_vat = input(&id, None);
        historical_vat.vat = VatRate::Vat20;
        let mut bad_barcode = input(&id, None);
        bad_barcode.barcodes = vec!["4600051000058".to_string()];
        for bad in [fractional_piece, historical_vat, bad_barcode] {
            let error = product_builder(bad).unwrap_err();
            assert!(error.starts_with("invalid:"), "{error}");
        }
    }

    #[test]
    fn negative_price_is_rejected() {
        // Optional fields may be omitted entirely by the caller.
        let parsed: CrmProductWriteInput = serde_json::from_value(json!({
            "channel_id": uuid::Uuid::new_v4().to_string(),
            "product_id": uuid::Uuid::new_v4().to_string(),
            "name": "x",
            "unit": "piece",
            "price_minor": -1,
            "vat": "none",
            "payment_object": "service",
        }))
        .unwrap();
        assert!(product_builder(parsed).unwrap_err().starts_with("invalid:"));
    }

    #[test]
    fn list_path_encodes_only_present_filters() {
        assert_eq!(
            products_path(None, None, false, None, None),
            "/crm/products"
        );
        assert_eq!(
            products_path(Some("  "), Some(""), false, None, Some("")),
            "/crm/products"
        );
        assert_eq!(
            products_path(
                Some("Молоко 3,2% & сыр"),
                None,
                true,
                Some(50),
                Some("c+/=")
            ),
            "/crm/products?q=%D0%9C%D0%BE%D0%BB%D0%BE%D0%BA%D0%BE+3%2C2%25+%26+%D1%81%D1%8B%D1%80\
             &archived=true&limit=50&after=c%2B%2F%3D"
        );
        assert_eq!(
            products_path(None, Some(" 4600051000057 "), false, Some(0), None),
            "/crm/products?barcode=4600051000057&limit=1"
        );
        assert_eq!(
            products_path(None, None, false, Some(5_000), None),
            "/crm/products?limit=200"
        );
    }
}
