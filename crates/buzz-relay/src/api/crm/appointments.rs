//! `GET /crm/appointments`, `GET /crm/appointments/{id}` and `GET /crm/slots`
//! — the booking journal and the free start times for a set of services.

use std::sync::Arc;

use axum::{
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use chrono::{NaiveDate, NaiveDateTime, Timelike};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use buzz_crm::appointment::free_starts;
use buzz_crm::{format_clock, WorkInterval};
use buzz_db::crm::{AppointmentQuery, SlotInputs, MAX_APPOINTMENT_RANGE_DAYS};

use super::authorize_space_member;
use crate::{
    api::{api_error, internal_error, not_found, parse_query_or_400},
    state::AppState,
};

/// Route path of the journal.
pub const APPOINTMENTS_PATH: &str = "/crm/appointments";
/// Route path of one appointment.
pub const APPOINTMENT_PATH: &str = "/crm/appointments/{appointment_id}";
/// Route path of the slot picker.
pub const SLOTS_PATH: &str = "/crm/slots";

/// Most services one slot query may combine.
const MAX_SLOT_SERVICES: usize = 20;
/// Default slot grid, minutes.
const DEFAULT_STEP_MIN: u16 = 15;

/// Query parameters of `GET /crm/appointments`.
#[derive(Debug, Deserialize, Default)]
pub struct AppointmentParams {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    staff_id: Option<Uuid>,
    customer_id: Option<Uuid>,
}

/// Query parameters of `GET /crm/slots`.
#[derive(Debug, Deserialize, Default)]
pub struct SlotParams {
    pub(crate) date: Option<NaiveDate>,
    /// Comma-separated product ids, in the order they are performed.
    pub(crate) service_ids: Option<String>,
    pub(crate) staff_id: Option<Uuid>,
    pub(crate) step: Option<u16>,
}

fn appointment_query(params: &AppointmentParams) -> Result<AppointmentQuery, &'static str> {
    let query = AppointmentQuery {
        from: params.from,
        to: params.to,
        staff_id: params.staff_id,
        customer_id: params.customer_id,
    };
    // A client's history may span any time; the journal is read by range.
    if params.customer_id.is_some() && params.from.is_none() && params.to.is_none() {
        return Ok(query);
    }
    let (Some(from), Some(to)) = (params.from, params.to) else {
        return Err("from and to are required unless customer_id is given");
    };
    if from > to {
        return Err("from must not be after to");
    }
    if (to - from).num_days() >= MAX_APPOINTMENT_RANGE_DAYS {
        return Err("the range must be at most 93 days");
    }
    Ok(query)
}

pub(crate) struct SlotRequest {
    pub(crate) date: NaiveDate,
    pub(crate) service_ids: Vec<Uuid>,
    pub(crate) staff_id: Option<Uuid>,
    pub(crate) step: u16,
}

pub(crate) fn slot_request(params: &SlotParams) -> Result<SlotRequest, String> {
    let date = params.date.ok_or("date is required")?;
    let raw = params.service_ids.as_deref().unwrap_or_default();
    let mut service_ids = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let id: Uuid = part
            .parse()
            .map_err(|_| format!("service_ids: {part:?} is not a UUID"))?;
        if !service_ids.contains(&id) {
            service_ids.push(id);
        }
    }
    if service_ids.is_empty() || service_ids.len() > MAX_SLOT_SERVICES {
        return Err(format!(
            "service_ids must list 1–{MAX_SLOT_SERVICES} services"
        ));
    }
    let step = params.step.unwrap_or(DEFAULT_STEP_MIN);
    if !(5..=60).contains(&step) || !step.is_multiple_of(5) {
        return Err("step must be 5–60 minutes in steps of 5".into());
    }
    Ok(SlotRequest {
        date,
        service_ids,
        staff_id: params.staff_id,
        step,
    })
}

pub(crate) fn intervals_of(value: &Value) -> Vec<WorkInterval> {
    value
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|interval| {
                    Some(WorkInterval {
                        start: u16::try_from(interval["start"].as_u64()?).ok()?,
                        end: u16::try_from(interval["end"].as_u64()?).ok()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Free starts of every capable staff member no earlier than `earliest`
/// (space-local time), or a client error.
pub(crate) fn slots_json(
    request: &SlotRequest,
    inputs: &SlotInputs,
    earliest: NaiveDateTime,
) -> Result<Value, String> {
    if inputs.services.len() != request.service_ids.len() {
        let missing = request
            .service_ids
            .iter()
            .find(|id| !inputs.services.iter().any(|s| s.0 == **id))
            .map_or_else(String::new, Uuid::to_string);
        return Err(format!("service_ids: {missing} is not an active service"));
    }
    let mut duration: u32 = 0;
    let mut total: i64 = 0;
    for (id, _, minutes, price) in &inputs.services {
        let minutes = minutes.ok_or_else(|| format!("service {id} has no duration"))?;
        duration += u32::try_from(minutes).unwrap_or(0);
        total += price;
    }
    let not_before = if request.date < earliest.date() {
        None
    } else if request.date == earliest.date() {
        let at = earliest.time();
        u16::try_from(at.hour() * 60 + at.minute() + 1).ok()
    } else {
        Some(0)
    };
    let duration_u16 = u16::try_from(duration).unwrap_or(u16::MAX);
    let staff: Vec<Value> = inputs
        .staff
        .iter()
        .map(|(staff_id, name, intervals)| {
            let busy: Vec<(u16, u16)> = inputs
                .busy
                .iter()
                .filter(|(id, _, _)| id == staff_id)
                .filter_map(|(_, s, e)| Some((u16::try_from(*s).ok()?, u16::try_from(*e).ok()?)))
                .collect();
            let starts = not_before.map_or_else(Vec::new, |not_before| {
                free_starts(
                    &intervals_of(intervals),
                    &busy,
                    duration_u16,
                    request.step,
                    not_before,
                )
            });
            json!({
                "staff_id": staff_id,
                "name": name,
                "starts": starts.into_iter().map(format_clock).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({
        "date": request.date,
        "duration_min": duration,
        "total_minor": total,
        "step": request.step,
        "staff": staff,
    }))
}

/// `GET /crm/appointments?from&to[&staff_id][&customer_id]` — the journal
/// (at most 93 days), or a client's whole history with `customer_id` alone.
pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, APPOINTMENTS_PATH, raw_query.as_deref())
            .await
        {
            Ok(member) => member,
            Err(response) => return response,
        };
    let params: AppointmentParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    let query = match appointment_query(&params) {
        Ok(query) => query,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, message).into_response(),
    };
    match state
        .db
        .crm_list_appointments(member.community(), member.space.channel_id, &query)
        .await
    {
        Ok(appointments) => Json(json!({ "appointments": appointments })).into_response(),
        Err(e) => internal_error(&format!("CRM appointments read: {e}")).into_response(),
    }
}

/// `GET /crm/appointments/{id}`.
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(appointment_id): Path<Uuid>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let path = format!("{APPOINTMENTS_PATH}/{appointment_id}");
    let member = match authorize_space_member(&state, &headers, &path, raw_query.as_deref()).await {
        Ok(member) => member,
        Err(response) => return response,
    };
    match state
        .db
        .crm_get_appointment(member.community(), member.space.channel_id, appointment_id)
        .await
    {
        Ok(Some(appointment)) => Json::<Value>(json!(appointment)).into_response(),
        Ok(None) => not_found("appointment not found").into_response(),
        Err(e) => internal_error(&format!("CRM appointment read: {e}")).into_response(),
    }
}

/// `GET /crm/slots?date&service_ids=a,b[&staff_id][&step]` — free start
/// times of every active staff member who performs all the services.
pub async fn slots(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, SLOTS_PATH, raw_query.as_deref()).await {
            Ok(member) => member,
            Err(response) => return response,
        };
    let params: SlotParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    let request = match slot_request(&params) {
        Ok(request) => request,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, &message).into_response(),
    };
    let inputs = match state
        .db
        .crm_slot_inputs(
            member.community(),
            member.space.channel_id,
            request.date,
            &request.service_ids,
            request.staff_id,
        )
        .await
    {
        Ok(Some(inputs)) => inputs,
        Ok(None) => return not_found("CRM space not found").into_response(),
        Err(e) => return internal_error(&format!("CRM slots read: {e}")).into_response(),
    };
    match slots_json(&request, &inputs, inputs.now_local) {
        Ok(body) => Json(body).into_response(),
        Err(message) => api_error(StatusCode::BAD_REQUEST, &message).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(now: &str) -> SlotInputs {
        let olga = Uuid::from_u128(1);
        SlotInputs {
            staff: vec![(
                olga,
                "Olga".into(),
                json!([{"start": 600, "end": 840}, {"start": 900, "end": 1200}]),
            )],
            busy: vec![(olga, 600, 660)],
            services: vec![(Uuid::from_u128(9), "Haircut".into(), Some(60), 150_000)],
            now_local: now.parse().unwrap(),
        }
    }

    fn at(now: &str) -> NaiveDateTime {
        now.parse().unwrap()
    }

    fn request(date: &str, step: u16) -> SlotRequest {
        SlotRequest {
            date: date.parse().unwrap(),
            service_ids: vec![Uuid::from_u128(9)],
            staff_id: None,
            step,
        }
    }

    #[test]
    fn slots_skip_busy_time_and_the_past() {
        let body = slots_json(
            &request("2026-11-02", 60),
            &inputs("2026-11-01T12:00:00"),
            at("2026-11-01T12:00:00"),
        )
        .unwrap();
        assert_eq!(
            body["staff"][0]["starts"],
            json!(["11:00", "12:00", "13:00", "15:00", "16:00", "17:00", "18:00", "19:00"])
        );
        assert_eq!(body["duration_min"], json!(60));
        let today = slots_json(
            &request("2026-11-02", 60),
            &inputs("2026-11-02T15:10:00"),
            at("2026-11-02T15:10:00"),
        )
        .unwrap();
        assert_eq!(
            today["staff"][0]["starts"],
            json!(["16:00", "17:00", "18:00", "19:00"])
        );
        let past = slots_json(
            &request("2026-11-02", 60),
            &inputs("2026-11-03T09:00:00"),
            at("2026-11-03T09:00:00"),
        )
        .unwrap();
        assert_eq!(past["staff"][0]["starts"], json!([]));
    }

    #[test]
    fn slot_requests_are_validated() {
        let params = |ids: &str, step: Option<u16>| SlotParams {
            date: Some("2026-11-02".parse().unwrap()),
            service_ids: Some(ids.into()),
            staff_id: None,
            step,
        };
        let id = Uuid::from_u128(9).to_string();
        assert!(slot_request(&params(&id, None)).is_ok());
        assert!(slot_request(&params("", None)).is_err());
        assert!(slot_request(&params("nope", None)).is_err());
        assert!(slot_request(&params(&id, Some(7))).is_err());
        let mut missing = inputs("2026-11-01T12:00:00");
        missing.services.clear();
        assert!(slots_json(
            &request("2026-11-02", 15),
            &missing,
            at("2026-11-01T12:00:00")
        )
        .is_err());
    }

    #[test]
    fn journal_reads_need_a_bounded_range_or_a_customer() {
        let params = |from: Option<&str>, to: Option<&str>, customer: bool| AppointmentParams {
            from: from.map(|d| d.parse().unwrap()),
            to: to.map(|d| d.parse().unwrap()),
            staff_id: None,
            customer_id: customer.then(|| Uuid::from_u128(5)),
        };
        assert!(appointment_query(&params(Some("2026-11-01"), Some("2026-11-07"), false)).is_ok());
        assert!(appointment_query(&params(None, None, true)).is_ok());
        assert!(appointment_query(&params(None, None, false)).is_err());
        assert!(appointment_query(&params(Some("2026-11-01"), Some("2027-03-01"), false)).is_err());
    }
}
