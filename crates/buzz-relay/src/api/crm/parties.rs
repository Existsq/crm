//! `GET /crm/parties` and `GET /crm/parties/{id}` — customers and suppliers —
//! and `GET /crm/loyalty`, their bonus programme and balances.

use std::sync::Arc;

use axum::{
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use buzz_db::crm::{PartyQuery, PartyRole, MAX_PARTY_PAGE};

use super::authorize_space_member;
use crate::{
    api::{api_error, internal_error, not_found, parse_query_or_400},
    state::AppState,
};

/// Route path of the party list.
pub const PARTIES_PATH: &str = "/crm/parties";
/// Route path of one party.
pub const PARTY_PATH: &str = "/crm/parties/{party_id}";
/// Route path of the loyalty read.
pub const LOYALTY_PATH: &str = "/crm/loyalty";

const DEFAULT_PARTY_LIMIT: i64 = 50;
/// Longest accepted search text, in characters.
const MAX_QUERY_CHARS: usize = 128;

/// Query parameters of `GET /crm/parties`.
#[derive(Debug, Deserialize, Default)]
pub struct PartiesParams {
    q: Option<String>,
    /// `customers` or `suppliers`; parties of type `both` match either.
    role: Option<String>,
    archived: Option<bool>,
    limit: Option<i64>,
    after: Option<Uuid>,
}

fn party_query(params: PartiesParams) -> Result<PartyQuery, &'static str> {
    let limit = params.limit.unwrap_or(DEFAULT_PARTY_LIMIT);
    if !(1..=MAX_PARTY_PAGE).contains(&limit) {
        return Err("limit must be between 1 and 200");
    }
    if params
        .q
        .as_deref()
        .is_some_and(|q| q.chars().count() > MAX_QUERY_CHARS)
    {
        return Err("q must be at most 128 characters");
    }
    let role = match params.role.as_deref() {
        None | Some("") => None,
        Some("customers") => Some(PartyRole::Customers),
        Some("suppliers") => Some(PartyRole::Suppliers),
        Some(_) => return Err("role must be customers or suppliers"),
    };
    Ok(PartyQuery {
        q: params.q,
        role,
        include_archived: params.archived.unwrap_or(false),
        after: params.after,
        limit,
    })
}

/// `GET /crm/parties` — one keyset page of the space's customers and
/// suppliers, each with its purchase totals.
///
/// `q` matches a name or email substring, an INN prefix or a fragment of the
/// phone digits; `role` narrows to customers or suppliers; `archived=true`
/// includes archived parties; `after` is the `next` cursor.
pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, PARTIES_PATH, raw_query.as_deref()).await {
            Ok(member) => member,
            Err(response) => return response,
        };
    let params: PartiesParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    let query = match party_query(params) {
        Ok(query) => query,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, message).into_response(),
    };
    match state
        .db
        .crm_list_parties(member.community(), member.space.channel_id, &query)
        .await
    {
        Ok(page) => Json(json!({ "parties": page.parties, "next": page.next })).into_response(),
        Err(e) => internal_error(&format!("CRM parties read: {e}")).into_response(),
    }
}

/// `GET /crm/parties/{id}` — one party, archived or not.
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(party_id): Path<Uuid>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let path = format!("{PARTIES_PATH}/{party_id}");
    let member = match authorize_space_member(&state, &headers, &path, raw_query.as_deref()).await {
        Ok(member) => member,
        Err(response) => return response,
    };
    match state
        .db
        .crm_get_party(member.community(), member.space.channel_id, party_id)
        .await
    {
        Ok(Some(party)) => Json::<Value>(json!(party)).into_response(),
        Ok(None) => not_found("party not found").into_response(),
        Err(e) => internal_error(&format!("CRM party read: {e}")).into_response(),
    }
}

/// Query parameters of `GET /crm/loyalty`.
#[derive(Debug, Deserialize, Default)]
pub struct LoyaltyParams {
    customer_id: Option<Uuid>,
}

/// `GET /crm/loyalty[?customer_id=]` — the bonus programme (all zero and off
/// until set) and, with `customer_id`, that customer's balance and history.
pub async fn loyalty(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, LOYALTY_PATH, raw_query.as_deref()).await {
            Ok(member) => member,
            Err(response) => return response,
        };
    let params: LoyaltyParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    let space = member.space.channel_id;
    let settings = match state
        .db
        .crm_loyalty_settings(member.community(), space)
        .await
    {
        Ok(settings) => settings.unwrap_or(buzz_db::crm::loyalty::LoyaltySettingsRecord {
            enabled: false,
            cashback_bp: 0,
            max_redeem_bp: 0,
        }),
        Err(e) => return internal_error(&format!("CRM loyalty read: {e}")).into_response(),
    };
    let mut body = json!({ "settings": settings });
    if let Some(customer_id) = params.customer_id {
        match state
            .db
            .crm_customer_bonuses(member.community(), space, customer_id)
            .await
        {
            Ok(bonuses) => body["customer"] = json!(bonuses),
            Err(e) => return internal_error(&format!("CRM bonuses read: {e}")).into_response(),
        }
    }
    Json(body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_and_role_are_validated() {
        let query = party_query(PartiesParams::default()).unwrap();
        assert_eq!(query.limit, DEFAULT_PARTY_LIMIT);
        assert_eq!(query.role, None);
        for bad in [0, MAX_PARTY_PAGE + 1] {
            let params = PartiesParams {
                limit: Some(bad),
                ..PartiesParams::default()
            };
            assert!(party_query(params).is_err(), "limit {bad}");
        }
        let params = PartiesParams {
            role: Some("suppliers".into()),
            ..PartiesParams::default()
        };
        assert_eq!(
            party_query(params).unwrap().role,
            Some(PartyRole::Suppliers)
        );
        let params = PartiesParams {
            role: Some("partners".into()),
            ..PartiesParams::default()
        };
        assert!(party_query(params).is_err());
        let params = PartiesParams {
            q: Some("я".repeat(MAX_QUERY_CHARS + 1)),
            ..PartiesParams::default()
        };
        assert!(party_query(params).is_err());
    }

    #[test]
    fn party_json_uses_wire_names() {
        let record = buzz_db::crm::CrmPartyRecord {
            party_id: Uuid::nil(),
            party_type: "customer".into(),
            name: "Мария".into(),
            phone: None,
            email: None,
            inn: None,
            address: None,
            note: None,
            archived: false,
            purchases: Default::default(),
            revision_event_id: vec![0xab; 32],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let value = json!(record);
        assert_eq!(value["id"], json!(Uuid::nil()));
        assert_eq!(value["type"], json!("customer"));
        assert_eq!(value["revision"], json!("ab".repeat(32)));
        assert_eq!(value["purchases"]["sales_count"], json!(0));
        assert!(value.get("party_type").is_none());
    }
}
