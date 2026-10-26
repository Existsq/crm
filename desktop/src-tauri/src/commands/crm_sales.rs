//! CRM customers, sales and analytics commands (relay kinds 42103–42105,
//! `GET /crm/parties`, `/crm/sales`, `/crm/analytics`).
//!
//! Reads return the relay's read model unchanged; writes build the event with
//! `buzz_sdk` (which runs the relay's payload validation), sign it with the
//! active identity and submit it over HTTP.

use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use buzz_crm_pkg::{
    Money, PartyPayload, PartyType, PaymentMethod, SaleLine, SalePayload, SalePayment,
};

use super::crm::{parse_uuid, sdk_error, CrmWriteWire};
use crate::{
    app_state::AppState,
    relay::{get_relay_json, submit_event},
};

/// Relay page-size bound for the CRM lists.
const MAX_PAGE: u32 = 200;

/// Party write input. Mirrors `CrmPartyWriteInput` in `tauriCrmSales.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmPartyWriteInput {
    pub channel_id: String,
    /// Party id, picked once per dialog for a create so a retry cannot
    /// create a second party.
    pub party_id: String,
    /// Current revision for an update; `None` creates the party.
    pub expected_revision: Option<String>,
    pub party_type: PartyType,
    pub name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub inn: Option<String>,
    pub address: Option<String>,
    pub note: Option<String>,
    #[serde(default)]
    pub archived: bool,
}

/// Sale input. Mirrors `CrmSaleInput` in `tauriCrmSales.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmSaleInput {
    pub channel_id: String,
    /// Sale id, picked once per sale dialog so a retried submit is a
    /// duplicate instead of a second sale.
    pub sale_id: String,
    pub warehouse_id: String,
    pub customer_id: Option<String>,
    pub payment: PaymentMethod,
    /// Split payment parts; empty pays the whole total with `payment`.
    #[serde(default)]
    pub payments: Vec<SalePayment>,
    /// Cash handed over, for the change.
    #[serde(default)]
    pub cash_tendered_minor: Option<Money>,
    pub lines: Vec<SaleLine>,
    pub note: Option<String>,
    /// Booked visit this sale pays for.
    #[serde(default)]
    pub appointment_id: Option<String>,
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
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

fn parties_path(
    query: Option<&str>,
    role: Option<&str>,
    archived: bool,
    limit: Option<u32>,
    after: Option<&str>,
) -> String {
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    if let Some(q) = non_blank(query) {
        params.append_pair("q", q);
    }
    if let Some(role) = non_blank(role) {
        params.append_pair("role", role);
    }
    if archived {
        params.append_pair("archived", "true");
    }
    if let Some(limit) = limit {
        params.append_pair("limit", &limit.clamp(1, MAX_PAGE).to_string());
    }
    if let Some(cursor) = non_blank(after) {
        params.append_pair("after", cursor);
    }
    with_query("/crm/parties", params)
}

fn sales_path(
    customer: Option<&str>,
    voided: bool,
    limit: Option<u32>,
    before: Option<i64>,
) -> String {
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    if let Some(customer) = non_blank(customer) {
        params.append_pair("customer", customer);
    }
    if !voided {
        params.append_pair("voided", "false");
    }
    if let Some(limit) = limit {
        params.append_pair("limit", &limit.clamp(1, MAX_PAGE).to_string());
    }
    if let Some(before) = before {
        params.append_pair("before", &before.to_string());
    }
    with_query("/crm/sales", params)
}

fn analytics_path(from: Option<&str>, to: Option<&str>) -> String {
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    if let Some(from) = non_blank(from) {
        params.append_pair("from", from);
    }
    if let Some(to) = non_blank(to) {
        params.append_pair("to", to);
    }
    with_query("/crm/analytics", params)
}

/// `GET /crm/parties` as returned by the relay (`{parties, next}`).
#[tauri::command]
pub async fn crm_list_parties(
    query: Option<String>,
    role: Option<String>,
    archived: Option<bool>,
    limit: Option<u32>,
    after: Option<String>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let path = parties_path(
        query.as_deref(),
        role.as_deref(),
        archived.unwrap_or(false),
        limit,
        after.as_deref(),
    );
    get_relay_json::<Value>(&state, &path).await
}

/// `GET /crm/parties/{id}` as returned by the relay.
#[tauri::command]
pub async fn crm_get_party(party_id: String, state: State<'_, AppState>) -> Result<Value, String> {
    let id = parse_uuid(&party_id, "party id")?;
    get_relay_json::<Value>(&state, &format!("/crm/parties/{id}")).await
}

/// Create or update a customer/supplier.
#[tauri::command]
pub async fn crm_write_party(
    input: CrmPartyWriteInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let (builder, party_id) = party_builder(input)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(party_id.to_string()),
    })
}

/// `GET /crm/sales` as returned by the relay (`{sales, next}`).
#[tauri::command]
pub async fn crm_list_sales(
    customer: Option<String>,
    voided: Option<bool>,
    limit: Option<u32>,
    before: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let path = sales_path(customer.as_deref(), voided.unwrap_or(true), limit, before);
    get_relay_json::<Value>(&state, &path).await
}

/// `GET /crm/sales/{id}` with lines, as returned by the relay.
#[tauri::command]
pub async fn crm_get_sale(sale_id: String, state: State<'_, AppState>) -> Result<Value, String> {
    let id = parse_uuid(&sale_id, "sale id")?;
    get_relay_json::<Value>(&state, &format!("/crm/sales/{id}")).await
}

/// Post a sale.
#[tauri::command]
pub async fn crm_post_sale(
    input: CrmSaleInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let (builder, sale_id) = sale_builder(input)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(sale_id.to_string()),
    })
}

/// Void (сторно) a sale.
#[tauri::command]
pub async fn crm_void_sale(
    channel_id: String,
    sale_id: String,
    reason: String,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let channel = parse_uuid(&channel_id, "channel id")?;
    let sale = parse_uuid(&sale_id, "sale id")?;
    let builder = buzz_sdk_pkg::build_crm_sale_void(channel, sale, &reason).map_err(sdk_error)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(sale.to_string()),
    })
}

/// `GET /crm/analytics` as returned by the relay.
#[tauri::command]
pub async fn crm_get_analytics(
    from: Option<String>,
    to: Option<String>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    get_relay_json::<Value>(&state, &analytics_path(from.as_deref(), to.as_deref())).await
}

fn party_builder(input: CrmPartyWriteInput) -> Result<(nostr::EventBuilder, uuid::Uuid), String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let party_id = parse_uuid(&input.party_id, "party id")?;
    let payload = PartyPayload {
        v: 1,
        party_type: input.party_type,
        name: input.name,
        phone: input.phone,
        email: input.email,
        inn: input.inn,
        address: input.address,
        note: input.note,
        archived: input.archived,
    };
    let builder = buzz_sdk_pkg::build_crm_party(
        channel,
        party_id,
        payload,
        input.expected_revision.as_deref(),
    )
    .map_err(sdk_error)?;
    Ok((builder, party_id))
}

fn sale_builder(input: CrmSaleInput) -> Result<(nostr::EventBuilder, uuid::Uuid), String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let sale_id = parse_uuid(&input.sale_id, "sale id")?;
    let payload = SalePayload {
        v: 1,
        warehouse_id: parse_uuid(&input.warehouse_id, "warehouse id")?,
        customer_id: non_blank(input.customer_id.as_deref())
            .map(|id| parse_uuid(id, "customer id"))
            .transpose()?,
        payment: input.payment,
        payments: input.payments,
        cash_tendered_minor: input.cash_tendered_minor,
        lines: input.lines,
        note: input.note,
        appointment_id: non_blank(input.appointment_id.as_deref())
            .map(|id| parse_uuid(id, "appointment id"))
            .transpose()?,
    };
    let builder = buzz_sdk_pkg::build_crm_sale(channel, sale_id, payload).map_err(sdk_error)?;
    Ok((builder, sale_id))
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

    fn sign(builder: nostr::EventBuilder) -> nostr::Event {
        builder.sign_with_keys(&nostr::Keys::generate()).unwrap()
    }

    #[test]
    fn party_create_and_update_use_the_client_id() {
        let id = uuid::Uuid::new_v4().to_string();
        let input = |revision: Option<String>| -> CrmPartyWriteInput {
            serde_json::from_value(json!({
                "channel_id": uuid::Uuid::new_v4().to_string(),
                "party_id": id,
                "expected_revision": revision,
                "party_type": "customer",
                "name": " Мария ",
                "phone": "+7 912 000-11-22",
                "email": null, "inn": null, "address": null, "note": null,
            }))
            .unwrap()
        };
        let (builder, built) = party_builder(input(None)).unwrap();
        let event = sign(builder);
        assert_eq!(event.kind.as_u16(), 42103);
        assert_eq!(built.to_string(), id);
        assert_eq!(tag(&event, "crm-id"), Some(id.clone()));
        assert_eq!(tag(&event, "expected-revision"), None);
        let content: Value = serde_json::from_str(&event.content).unwrap();
        assert_eq!(content["name"], "Мария");
        assert_eq!(content["type"], "customer");

        let (builder, _) = party_builder(input(Some("ab".repeat(32)))).unwrap();
        assert_eq!(
            tag(&sign(builder), "expected-revision"),
            Some("ab".repeat(32))
        );
        let mut bad = input(None);
        bad.inn = Some("123".into());
        assert!(party_builder(bad).unwrap_err().starts_with("invalid:"));
    }

    #[test]
    fn sale_builder_validates_lines_and_optional_customer() {
        let input = |qty: i64, customer: Option<&str>| -> CrmSaleInput {
            serde_json::from_value(json!({
                "channel_id": uuid::Uuid::new_v4().to_string(),
                "sale_id": uuid::Uuid::new_v4().to_string(),
                "warehouse_id": uuid::Uuid::new_v4().to_string(),
                "customer_id": customer,
                "payment": "cash",
                "lines": [{"product_id": uuid::Uuid::new_v4(), "qty_milli": qty, "price_minor": 8990}],
                "note": null,
            }))
            .unwrap()
        };
        let (builder, _) = sale_builder(input(2000, Some(""))).unwrap();
        let event = sign(builder);
        assert_eq!(event.kind.as_u16(), 42104);
        let content: Value = serde_json::from_str(&event.content).unwrap();
        assert!(content.get("customer_id").is_none());
        assert!(sale_builder(input(0, None))
            .unwrap_err()
            .starts_with("invalid:"));
        assert!(sale_builder(input(1000, Some("nope"))).is_err());
    }

    #[test]
    fn paths_encode_only_present_filters() {
        assert_eq!(
            parties_path(None, Some(""), false, None, None),
            "/crm/parties"
        );
        assert_eq!(
            parties_path(Some(" Мария "), Some("customers"), true, Some(500), Some("c")),
            "/crm/parties?q=%D0%9C%D0%B0%D1%80%D0%B8%D1%8F&role=customers&archived=true&limit=200&after=c"
        );
        assert_eq!(sales_path(None, true, None, None), "/crm/sales");
        assert_eq!(
            sales_path(Some("x"), false, Some(0), Some(12)),
            "/crm/sales?customer=x&voided=false&limit=1&before=12"
        );
        assert_eq!(analytics_path(None, Some(" ")), "/crm/analytics");
        assert_eq!(
            analytics_path(Some("2026-10-01"), Some("2026-10-31")),
            "/crm/analytics?from=2026-10-01&to=2026-10-31"
        );
    }
}
