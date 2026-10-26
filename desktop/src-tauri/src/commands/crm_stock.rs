//! CRM stock commands (relay kinds 42106–42108, `GET /crm/stock*`,
//! `GET /crm/tech-cards`).
//!
//! Reads return the relay's read model unchanged; writes build the event with
//! `buzz_sdk` (which runs the relay's payload validation), sign it with the
//! active identity and submit it over HTTP.

use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use buzz_crm_pkg::{StockDocLine, StockDocPayload, StockDocType, TechCardItem, TechCardPayload};

use super::crm::{parse_uuid, sdk_error, CrmWriteWire};
use crate::{
    app_state::AppState,
    relay::{get_relay_json, submit_event},
};

/// Stock document input. Mirrors `CrmStockDocInput` in `tauriCrmStock.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmStockDocInput {
    pub channel_id: String,
    /// Picked once per dialog, so a retried submit is a relay duplicate.
    pub doc_id: String,
    pub doc_type: StockDocType,
    pub warehouse_id: uuid::Uuid,
    pub to_warehouse_id: Option<uuid::Uuid>,
    pub supplier_id: Option<uuid::Uuid>,
    pub reference: Option<String>,
    pub lines: Vec<StockDocLine>,
    pub note: Option<String>,
}

/// Tech card input. Mirrors `CrmTechCardInput` in `tauriCrmStock.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmTechCardInput {
    pub channel_id: String,
    pub service_id: String,
    /// Current revision to replace a card; `None` creates it.
    pub expected_revision: Option<String>,
    pub items: Vec<TechCardItem>,
}

fn with_query(path: &str, params: url::form_urlencoded::Serializer<'_, String>) -> String {
    let mut params = params;
    let encoded = params.finish();
    if encoded.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{encoded}")
    }
}

fn stock_path(
    warehouse_id: Option<&str>,
    query: Option<&str>,
    low: bool,
) -> Result<String, String> {
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    if let Some(id) = warehouse_id {
        params.append_pair("warehouse", &parse_uuid(id, "warehouse id")?.to_string());
    }
    if let Some(q) = query.map(str::trim).filter(|q| !q.is_empty()) {
        params.append_pair("q", q);
    }
    if low {
        params.append_pair("low", "true");
    }
    Ok(with_query("/crm/stock", params))
}

fn moves_path(
    warehouse_id: Option<&str>,
    product_id: Option<&str>,
    before: Option<i64>,
) -> Result<String, String> {
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    if let Some(id) = warehouse_id {
        params.append_pair("warehouse", &parse_uuid(id, "warehouse id")?.to_string());
    }
    if let Some(id) = product_id {
        params.append_pair("product", &parse_uuid(id, "product id")?.to_string());
    }
    if let Some(before) = before {
        params.append_pair("before", &before.to_string());
    }
    Ok(with_query("/crm/stock/moves", params))
}

fn docs_path(doc_type: Option<StockDocType>, before: Option<i64>) -> String {
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    if let Some(doc_type) = doc_type {
        params.append_pair("type", doc_type.as_str());
    }
    if let Some(before) = before {
        params.append_pair("before", &before.to_string());
    }
    with_query("/crm/stock/docs", params)
}

/// `GET /crm/stock` as returned by the relay (`{warehouse_id, items, totals}`).
#[tauri::command]
pub async fn crm_list_stock(
    warehouse_id: Option<String>,
    query: Option<String>,
    low: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let path = stock_path(
        warehouse_id.as_deref(),
        query.as_deref(),
        low.unwrap_or(false),
    )?;
    get_relay_json::<Value>(&state, &path).await
}

/// `GET /crm/stock/moves` as returned by the relay (`{moves, next}`).
#[tauri::command]
pub async fn crm_list_stock_moves(
    warehouse_id: Option<String>,
    product_id: Option<String>,
    before: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let path = moves_path(warehouse_id.as_deref(), product_id.as_deref(), before)?;
    get_relay_json::<Value>(&state, &path).await
}

/// `GET /crm/stock/docs` as returned by the relay (`{docs, next}`).
#[tauri::command]
pub async fn crm_list_stock_docs(
    doc_type: Option<StockDocType>,
    before: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    get_relay_json::<Value>(&state, &docs_path(doc_type, before)).await
}

/// `GET /crm/stock/docs/{id}` with lines.
#[tauri::command]
pub async fn crm_get_stock_doc(
    doc_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let id = parse_uuid(&doc_id, "document id")?;
    get_relay_json::<Value>(&state, &format!("/crm/stock/docs/{id}")).await
}

/// `GET /crm/tech-cards` as returned by the relay (`{cards}`).
#[tauri::command]
pub async fn crm_list_tech_cards(state: State<'_, AppState>) -> Result<Value, String> {
    get_relay_json::<Value>(&state, "/crm/tech-cards").await
}

/// Post a receipt, write-off, inventory or transfer.
#[tauri::command]
pub async fn crm_post_stock_doc(
    input: CrmStockDocInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let (builder, doc_id) = stock_doc_builder(input)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(doc_id.to_string()),
    })
}

/// Void (сторно) a stock document.
#[tauri::command]
pub async fn crm_void_stock_doc(
    channel_id: String,
    doc_id: String,
    reason: String,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let channel = parse_uuid(&channel_id, "channel id")?;
    let doc = parse_uuid(&doc_id, "document id")?;
    let builder =
        buzz_sdk_pkg::build_crm_stock_doc_void(channel, doc, &reason).map_err(sdk_error)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(doc.to_string()),
    })
}

/// Create or replace the tech card of a service.
#[tauri::command]
pub async fn crm_write_tech_card(
    input: CrmTechCardInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let (builder, service_id) = tech_card_builder(input)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(service_id.to_string()),
    })
}

fn stock_doc_builder(input: CrmStockDocInput) -> Result<(nostr::EventBuilder, uuid::Uuid), String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let doc_id = parse_uuid(&input.doc_id, "document id")?;
    let payload = StockDocPayload {
        v: 1,
        doc_type: input.doc_type,
        warehouse_id: input.warehouse_id,
        to_warehouse_id: input.to_warehouse_id,
        supplier_id: input.supplier_id,
        reference: input.reference,
        lines: input.lines,
        note: input.note,
    };
    let builder = buzz_sdk_pkg::build_crm_stock_doc(channel, doc_id, payload).map_err(sdk_error)?;
    Ok((builder, doc_id))
}

fn tech_card_builder(input: CrmTechCardInput) -> Result<(nostr::EventBuilder, uuid::Uuid), String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let service_id = parse_uuid(&input.service_id, "service id")?;
    let builder = buzz_sdk_pkg::build_crm_tech_card(
        channel,
        service_id,
        TechCardPayload {
            v: 1,
            items: input.items,
        },
        input.expected_revision.as_deref(),
    )
    .map_err(sdk_error)?;
    Ok((builder, service_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tag(event: &nostr::Event, name: &str) -> Option<String> {
        event
            .tags
            .iter()
            .find(|t| t.as_slice().first().map(String::as_str) == Some(name))
            .and_then(|t| t.as_slice().get(1).cloned())
    }

    #[test]
    fn stock_doc_builder_validates_the_document() {
        let id = uuid::Uuid::new_v4().to_string();
        let input = |doc_type: &str, cost: Value| -> CrmStockDocInput {
            serde_json::from_value(json!({
                "channel_id": uuid::Uuid::new_v4().to_string(),
                "doc_id": id,
                "doc_type": doc_type,
                "warehouse_id": uuid::Uuid::new_v4(),
                "to_warehouse_id": null,
                "supplier_id": null,
                "reference": null,
                "lines": [{"product_id": uuid::Uuid::new_v4(), "qty_milli": 2000, "cost_minor": cost}],
                "note": null,
            }))
            .unwrap()
        };
        let (builder, built) = stock_doc_builder(input("receipt", json!(15_000))).unwrap();
        let event = builder.sign_with_keys(&nostr::Keys::generate()).unwrap();
        assert_eq!(event.kind.as_u16(), 42106);
        assert_eq!(built.to_string(), id);
        assert_eq!(tag(&event, "crm-id"), Some(id.clone()));
        assert!(stock_doc_builder(input("receipt", Value::Null))
            .unwrap_err()
            .starts_with("invalid:"));
        assert!(stock_doc_builder(input("write_off", json!(1))).is_err());
    }

    #[test]
    fn tech_card_builder_carries_the_revision() {
        let input = |revision: Option<String>| -> CrmTechCardInput {
            serde_json::from_value(json!({
                "channel_id": uuid::Uuid::new_v4().to_string(),
                "service_id": uuid::Uuid::new_v4().to_string(),
                "expected_revision": revision,
                "items": [{"product_id": uuid::Uuid::new_v4(), "qty_milli": 30}],
            }))
            .unwrap()
        };
        let (builder, _) = tech_card_builder(input(Some("ab".repeat(32)))).unwrap();
        let event = builder.sign_with_keys(&nostr::Keys::generate()).unwrap();
        assert_eq!(event.kind.as_u16(), 42108);
        assert_eq!(tag(&event, "expected-revision"), Some("ab".repeat(32)));
        assert!(tech_card_builder(input(Some("x".into()))).is_err());
    }

    #[test]
    fn paths_encode_only_present_filters() {
        let warehouse = uuid::Uuid::new_v4().to_string();
        assert_eq!(stock_path(None, Some("  "), false).unwrap(), "/crm/stock");
        assert_eq!(
            stock_path(Some(&warehouse), Some("шампунь & co"), true).unwrap(),
            format!(
                "/crm/stock?warehouse={warehouse}&q=%D1%88%D0%B0%D0%BC%D0%BF%D1%83%D0%BD%D1%8C+%26+co&low=true"
            )
        );
        assert!(stock_path(Some("x&y"), None, false).is_err());
        assert_eq!(
            moves_path(None, None, Some(42)).unwrap(),
            "/crm/stock/moves?before=42"
        );
        assert_eq!(
            docs_path(Some(StockDocType::WriteOff), None),
            "/crm/stock/docs?type=write_off"
        );
    }
}
