//! `GET /crm/staff`, `GET /crm/staff/{id}` and `GET /crm/schedule` — the
//! people clients book and their working days.

use std::sync::Arc;

use axum::{
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use buzz_db::crm::MAX_SCHEDULE_RANGE_DAYS;

use super::authorize_space_member;
use crate::{
    api::{api_error, internal_error, not_found, parse_query_or_400},
    state::AppState,
};

/// Route path of the staff list.
pub const STAFF_PATH: &str = "/crm/staff";
/// Route path of one staff member.
pub const STAFF_MEMBER_PATH: &str = "/crm/staff/{staff_id}";
/// Route path of the working days.
pub const SCHEDULE_PATH: &str = "/crm/schedule";

/// Query parameters of `GET /crm/staff`.
#[derive(Debug, Deserialize, Default)]
pub struct StaffParams {
    archived: Option<bool>,
}

/// Query parameters of `GET /crm/schedule`.
#[derive(Debug, Deserialize, Default)]
pub struct ScheduleParams {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
}

fn schedule_range(params: &ScheduleParams) -> Result<(NaiveDate, NaiveDate), &'static str> {
    let (Some(from), Some(to)) = (params.from, params.to) else {
        return Err("from and to are required");
    };
    if from > to {
        return Err("from must not be after to");
    }
    if (to - from).num_days() >= MAX_SCHEDULE_RANGE_DAYS {
        return Err("the range must be at most 93 days");
    }
    Ok((from, to))
}

/// `GET /crm/staff` — every staff member of the space; `archived=true`
/// includes archived ones.
pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, STAFF_PATH, raw_query.as_deref()).await {
            Ok(member) => member,
            Err(response) => return response,
        };
    let params: StaffParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    match state
        .db
        .crm_list_staff(
            member.community(),
            member.space.channel_id,
            params.archived.unwrap_or(false),
        )
        .await
    {
        Ok(staff) => Json(json!({ "staff": staff })).into_response(),
        Err(e) => internal_error(&format!("CRM staff read: {e}")).into_response(),
    }
}

/// `GET /crm/staff/{id}` — one staff member, archived or not.
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(staff_id): Path<Uuid>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let path = format!("{STAFF_PATH}/{staff_id}");
    let member = match authorize_space_member(&state, &headers, &path, raw_query.as_deref()).await {
        Ok(member) => member,
        Err(response) => return response,
    };
    match state
        .db
        .crm_get_staff(member.community(), member.space.channel_id, staff_id)
        .await
    {
        Ok(Some(staff)) => Json::<Value>(json!(staff)).into_response(),
        Ok(None) => not_found("staff member not found").into_response(),
        Err(e) => internal_error(&format!("CRM staff read: {e}")).into_response(),
    }
}

/// `GET /crm/schedule?from=YYYY-MM-DD&to=YYYY-MM-DD` — scheduled days of
/// every staff member (at most 93 days). Days without a row are unscheduled.
pub async fn schedule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, SCHEDULE_PATH, raw_query.as_deref()).await
        {
            Ok(member) => member,
            Err(response) => return response,
        };
    let params: ScheduleParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    let (from, to) = match schedule_range(&params) {
        Ok(range) => range,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, message).into_response(),
    };
    match state
        .db
        .crm_list_staff_days(member.community(), member.space.channel_id, from, to)
        .await
    {
        Ok(days) => Json(json!({ "from": from, "to": to, "days": days })).into_response(),
        Err(e) => internal_error(&format!("CRM schedule read: {e}")).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(from: &str, to: &str) -> Result<(NaiveDate, NaiveDate), &'static str> {
        schedule_range(&ScheduleParams {
            from: Some(from.parse().unwrap()),
            to: Some(to.parse().unwrap()),
        })
    }

    #[test]
    fn schedule_ranges_are_bounded() {
        assert!(range("2026-10-01", "2026-10-31").is_ok());
        assert!(range("2026-10-02", "2026-10-01").is_err());
        assert!(range("2026-10-01", "2027-01-01").is_ok());
        assert!(range("2026-10-01", "2027-01-02").is_err());
        assert!(schedule_range(&ScheduleParams::default()).is_err());
    }

    #[test]
    fn staff_json_uses_wire_names() {
        let record = buzz_db::crm::CrmStaffRecord {
            staff_id: Uuid::nil(),
            name: "Анна".into(),
            position: None,
            phone: None,
            service_ids: vec![Uuid::nil()],
            member: Some(vec![0xcd; 32]),
            archived: false,
            revision_event_id: vec![0xab; 32],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let value = json!(record);
        assert_eq!(value["id"], json!(Uuid::nil()));
        assert_eq!(value["member"], json!("cd".repeat(32)));
        assert_eq!(value["revision"], json!("ab".repeat(32)));
    }
}
