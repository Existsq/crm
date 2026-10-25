//! `GET /crm/online-booking` — the space's public booking page settings.

use std::sync::Arc;

use axum::{
    extract::{RawQuery, State},
    http::HeaderMap,
    response::{IntoResponse, Json, Response},
};
use serde_json::json;

use super::authorize_space_member;
use crate::{api::internal_error, api::public_booking::PAGE_PATH, state::AppState};

/// Route path of the settings read.
pub const ONLINE_BOOKING_PATH: &str = "/crm/online-booking";

/// `GET /crm/online-booking` — `{"settings", "revision", "public_path"}`;
/// `settings` and `revision` are `null` until the first kind 42135.
pub async fn get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, ONLINE_BOOKING_PATH, raw_query.as_deref())
            .await
        {
            Ok(member) => member,
            Err(response) => return response,
        };
    match state
        .db
        .crm_get_online_booking(member.community(), member.space.channel_id)
        .await
    {
        Ok(record) => {
            let (settings, revision) = record.map_or((None, None), |record| {
                (
                    Some(record.settings),
                    Some(hex::encode(record.revision_event_id)),
                )
            });
            Json(json!({
                "settings": settings,
                "revision": revision,
                "public_path": PAGE_PATH,
            }))
            .into_response()
        }
        Err(e) => internal_error(&format!("CRM online booking read: {e}")).into_response(),
    }
}
