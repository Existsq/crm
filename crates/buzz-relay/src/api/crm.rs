//! Authorized structured reads for the CRM (docs/crm/adr/0001).
//!
//! CRM projections (space settings, warehouses, later stock and analytics) are
//! relay-owned rows derived from signed CRM events. These endpoints expose
//! them to members of the CRM space channel; everyone else learns at most that
//! a space exists.

use std::sync::Arc;

use axum::{
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde_json::{json, Value};

use buzz_auth::NipFiMode;
use buzz_core::TenantContext;
use buzz_crm::CrmRole;

use crate::{
    api::{api_error, bridge, internal_error, parse_query_or_400},
    nip_fi_http::admit_nip_fi_http_on_state,
    state::AppState,
};

pub mod appointments;
pub mod parties;
pub mod products;
pub mod sales;
pub mod staff;
pub mod stock;

/// Route path of the space read.
pub const SPACE_PATH: &str = "/crm/space";

/// An authenticated relay member making a CRM read.
pub(crate) struct CrmReader {
    pub(crate) tenant: TenantContext,
    pub(crate) pubkey: nostr::PublicKey,
}

fn request_path(path: &str, raw_query: Option<&str>) -> String {
    match raw_query {
        Some(query) if !query.is_empty() => format!("{path}?{query}"),
        _ => path.to_string(),
    }
}

/// Bind the tenant from the host, run NIP-98/NIP-FI admission, replay and
/// relay-membership checks — the same sequence as the workflow read API.
#[allow(clippy::result_large_err)] // Response is the natural error type for axum handlers
pub(crate) async fn authorize_crm_reader(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    path: &str,
    raw_query: Option<&str>,
) -> Result<CrmReader, Response> {
    let raw_host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let tenant = crate::tenant::bind_community(&state.db, raw_host)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::NOT_FOUND,
                "relay: no community is configured for this host",
            )
            .into_response()
        })?;

    let path_with_query = request_path(path, raw_query);
    let url = bridge::nip98_expected_url(&state.config.relay_url, &tenant, &path_with_query);
    let nip_fi_active = !matches!(state.config.nip_fi.mode, NipFiMode::Off);
    let require_auth = state.config.require_auth_token || nip_fi_active;
    let admission = admit_nip_fi_http_on_state(
        state,
        headers,
        bridge::make_nip98_closure_for_admission(
            headers.clone(),
            "GET",
            url,
            None,
            require_auth,
            false,
        ),
    )?;
    let pubkey = *admission.proven_pubkey();
    let (event_id_bytes, signed_created_at) = admission.into_extra();

    bridge::enforce_http_admission(state, &tenant, &pubkey)
        .await
        .map_err(|e| e.into_response())?;
    bridge::check_nip98_replay(state, &tenant, event_id_bytes)
        .await
        .map_err(|e| e.into_response())?;

    let pubkey_bytes = pubkey.to_bytes().to_vec();
    let auth_tag = super::relay_members::extract_auth_tag_header(headers);
    super::relay_members::enforce_relay_membership(
        state,
        tenant.community(),
        &pubkey_bytes,
        auth_tag,
        signed_created_at,
    )
    .await
    .map_err(|e| e.into_response())?;

    Ok(CrmReader { tenant, pubkey })
}

/// The caller's active role in the space channel, read fresh (uncached).
#[allow(clippy::result_large_err)] // Response is the natural error type for axum handlers
pub(crate) async fn space_role(
    state: &Arc<AppState>,
    reader: &CrmReader,
    channel_id: uuid::Uuid,
) -> Result<Option<CrmRole>, Response> {
    let role = state
        .db
        .get_member_role(
            reader.tenant.community(),
            channel_id,
            &reader.pubkey.to_bytes(),
        )
        .await
        .map_err(|e| internal_error(&format!("CRM space role lookup: {e}")).into_response())?;
    role.map(|role| {
        role.parse::<CrmRole>()
            .map_err(|e| internal_error(&format!("CRM space role: {e}")).into_response())
    })
    .transpose()
}

/// A reader who is a current member of the CRM space channel.
pub(crate) struct CrmMember {
    pub(crate) reader: CrmReader,
    pub(crate) space: buzz_db::crm::CrmSpaceRecord,
}

impl CrmMember {
    pub(crate) fn community(&self) -> buzz_core::CommunityId {
        self.reader.tenant.community()
    }
}

/// Authenticate the caller and require membership of the CRM space channel:
/// 404 when the community has no space, 403 for non-members.
#[allow(clippy::result_large_err)] // Response is the natural error type for axum handlers
pub(crate) async fn authorize_space_member(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    path: &str,
    raw_query: Option<&str>,
) -> Result<CrmMember, Response> {
    let reader = authorize_crm_reader(state, headers, path, raw_query).await?;
    let space = state
        .db
        .crm_get_space(reader.tenant.community())
        .await
        .map_err(|e| internal_error(&format!("CRM space read: {e}")).into_response())?
        .ok_or_else(|| {
            api_error(StatusCode::NOT_FOUND, "this community has no CRM space").into_response()
        })?;
    if space_role(state, &reader, space.channel_id)
        .await?
        .is_none()
    {
        return Err(
            api_error(StatusCode::FORBIDDEN, "not a member of the CRM space").into_response(),
        );
    }
    Ok(CrmMember { reader, space })
}

/// Query parameters of `GET /crm/space`.
#[derive(Debug, serde::Deserialize, Default)]
pub struct SpaceParams {
    /// Include archived warehouses (to restore one).
    archived: Option<bool>,
}

/// `GET /crm/space` — the community's CRM space as the caller may see it.
///
/// - no space: `{"exists": false, "can_create": bool}`
/// - space, caller not a member: `{"exists": true, "member": false}`
/// - member: settings, revision, role, capabilities and live warehouses
///   (`?archived=true` adds archived ones).
pub async fn space(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let reader =
        match authorize_crm_reader(&state, &headers, SPACE_PATH, raw_query.as_deref()).await {
            Ok(reader) => reader,
            Err(response) => return response,
        };
    // Parse after admission so malformed parameters cannot pre-empt the
    // authentication contract (same order as the workflow reads).
    let params: SpaceParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    let community = reader.tenant.community();

    let space = match state.db.crm_get_space(community).await {
        Ok(space) => space,
        Err(e) => return internal_error(&format!("CRM space read: {e}")).into_response(),
    };
    let Some(space) = space else {
        let relay_role = match state
            .db
            .get_relay_member(community, &reader.pubkey.to_hex())
            .await
        {
            Ok(member) => member.map(|m| m.role),
            Err(e) => return internal_error(&format!("CRM relay role: {e}")).into_response(),
        };
        let can_create = matches!(relay_role.as_deref(), Some("owner" | "admin"));
        return Json(json!({ "exists": false, "can_create": can_create })).into_response();
    };

    let role = match space_role(&state, &reader, space.channel_id).await {
        Ok(role) => role,
        Err(response) => return response,
    };
    let Some(role) = role else {
        return Json(json!({ "exists": true, "member": false })).into_response();
    };

    let warehouses = match state
        .db
        .crm_list_warehouses(
            community,
            space.channel_id,
            params.archived.unwrap_or(false),
        )
        .await
    {
        Ok(rows) => rows,
        Err(e) => return internal_error(&format!("CRM warehouses read: {e}")).into_response(),
    };
    Json(space_body(&space, role, &warehouses)).into_response()
}

fn space_body(
    space: &buzz_db::crm::CrmSpaceRecord,
    role: CrmRole,
    warehouses: &[buzz_db::crm::CrmWarehouseRecord],
) -> Value {
    let capabilities: Vec<&str> = buzz_crm::capabilities(role)
        .into_iter()
        .map(|action| action.as_str())
        .collect();
    json!({
        "exists": true,
        "member": true,
        "channel_id": space.channel_id,
        "settings": space.settings,
        "revision": hex::encode(&space.revision_event_id),
        "my_role": role.as_str(),
        "capabilities": capabilities,
        "warehouses": warehouses.iter().map(|w| json!({
            "id": w.warehouse_id,
            "name": w.name,
            "is_default": w.is_default,
            "archived": w.archived,
            "revision": hex::encode(&w.revision_event_id),
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn member_body_carries_role_capabilities_and_hex_revisions() {
        let channel = Uuid::new_v4();
        let warehouse = Uuid::new_v4();
        let space = buzz_db::crm::CrmSpaceRecord {
            channel_id: channel,
            settings: json!({"currency": "RUB"}),
            revision_event_id: vec![0xab; 32],
            created_by: vec![1; 32],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let rows = vec![buzz_db::crm::CrmWarehouseRecord {
            warehouse_id: warehouse,
            name: "Склад".into(),
            is_default: true,
            archived: false,
            revision_event_id: vec![0xcd; 32],
        }];
        let body = space_body(&space, CrmRole::Member, &rows);
        assert_eq!(body["channel_id"], json!(channel));
        assert_eq!(body["revision"], json!("ab".repeat(32)));
        assert_eq!(body["my_role"], json!("member"));
        assert_eq!(
            body["capabilities"],
            json!([
                "read",
                "edit_catalog",
                "edit_parties",
                "post_documents",
                "record_fiscal"
            ])
        );
        assert_eq!(body["warehouses"][0]["id"], json!(warehouse));
        assert_eq!(body["warehouses"][0]["revision"], json!("cd".repeat(32)));
        assert!(
            body.get("created_by").is_none(),
            "creator pubkey is not exposed"
        );
    }
}

#[cfg(test)]
mod postgres_tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use base64::Engine;
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use tower::ServiceExt;
    use uuid::Uuid;

    fn proof(key: &Keys, host: &str, path_and_query: &str) -> String {
        let auth = EventBuilder::new(Kind::Custom(27235), "")
            .tags([
                Tag::parse(["u", &format!("https://{host}{path_and_query}")]).unwrap(),
                Tag::parse(["method", "GET"]).unwrap(),
                Tag::parse(["nonce", &Uuid::new_v4().to_string()]).unwrap(),
            ])
            .sign_with_keys(key)
            .unwrap();
        format!(
            "Nostr {}",
            base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&auth).unwrap())
        )
    }

    async fn get(
        state: &Arc<AppState>,
        host: &str,
        path_and_query: &str,
        authorization: Option<String>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method("GET")
            .uri(path_and_query)
            .header("host", host);
        if let Some(authorization) = authorization {
            request = request.header("authorization", authorization);
        }
        let response = crate::router::build_router(state.clone())
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn space_read_requires_real_auth_and_relay_membership() {
        let fixture = crate::api::bridge::postgres_tests::bridge_handler_test_state()
            .await
            .expect("isolated Postgres/Redis fixture");
        let mut state = (*fixture).clone();
        let config = Arc::make_mut(&mut state.config);
        config.require_auth_token = true;
        config.require_relay_membership = true;
        let state = Arc::new(state);
        let host = format!("crm-space-{}.local", Uuid::new_v4().simple());
        let community = state
            .db
            .ensure_configured_community(&host)
            .await
            .unwrap()
            .id;
        let key = Keys::generate();

        let anonymous = get(&state, &host, SPACE_PATH, None).await;
        assert_eq!(anonymous.0, StatusCode::UNAUTHORIZED, "{}", anonymous.1);

        let denied = get(
            &state,
            &host,
            SPACE_PATH,
            Some(proof(&key, &host, SPACE_PATH)),
        )
        .await;
        assert_eq!(denied.0, StatusCode::FORBIDDEN, "{}", denied.1);
        assert_eq!(denied.1["error"], "relay_membership_required");

        state
            .db
            .add_relay_member(community, &key.public_key().to_hex(), "member", None)
            .await
            .unwrap();
        let admitted = get(
            &state,
            &host,
            SPACE_PATH,
            Some(proof(&key, &host, SPACE_PATH)),
        )
        .await;
        assert_eq!(admitted.0, StatusCode::OK, "{}", admitted.1);
        assert_eq!(admitted.1, json!({"exists": false, "can_create": false}));

        // Query validation runs after admission.
        let bad = "/crm/space?archived=maybe";
        let rejected = get(&state, &host, bad, Some(proof(&key, &host, bad))).await;
        assert_eq!(rejected.0, StatusCode::BAD_REQUEST, "{}", rejected.1);
        let anonymous_bad = get(&state, &host, bad, None).await;
        assert_eq!(anonymous_bad.0, StatusCode::UNAUTHORIZED);
    }
}
