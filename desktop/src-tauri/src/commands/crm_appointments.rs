//! CRM appointment journal commands (relay kind 42133,
//! `GET /crm/appointments`, `GET /crm/slots`).
//!
//! Reads return the relay's read model unchanged; writes build the event with
//! `buzz_sdk` (which runs the relay's payload validation), sign it with the
//! active identity and submit it over HTTP.

use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use buzz_crm_pkg::{
    parse_clock, AppointmentPayload, AppointmentService, AppointmentSource, AppointmentStatus,
};

use super::crm::{parse_uuid, sdk_error, CrmWriteWire};
use crate::{
    app_state::AppState,
    relay::{get_relay_json, submit_event},
};

/// Appointment write input. Mirrors `CrmAppointmentWriteInput` in
/// `tauriCrmAppointments.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmAppointmentWriteInput {
    pub channel_id: String,
    /// Appointment id, picked once per dialog for a create so a retry cannot
    /// book twice.
    pub appointment_id: String,
    /// Current revision for an update; `None` creates the appointment.
    pub expected_revision: Option<String>,
    pub staff_id: uuid::Uuid,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// `HH:MM`.
    pub start: String,
    pub services: Vec<AppointmentService>,
    pub customer_id: Option<uuid::Uuid>,
    pub client_name: Option<String>,
    pub client_phone: Option<String>,
    pub status: AppointmentStatus,
    pub note: Option<String>,
    #[serde(default)]
    pub outside_hours: bool,
}

/// `GET /crm/appointments` as returned by the relay (`{appointments}`): the
/// journal for `from..=to`, or one client's history with `customer_id`.
#[tauri::command]
pub async fn crm_list_appointments(
    from: Option<String>,
    to: Option<String>,
    staff_id: Option<String>,
    customer_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let path = appointments_path(
        from.as_deref(),
        to.as_deref(),
        staff_id.as_deref(),
        customer_id.as_deref(),
    )?;
    get_relay_json::<Value>(&state, &path).await
}

/// `GET /crm/slots` as returned by the relay.
#[tauri::command]
pub async fn crm_get_slots(
    date: String,
    service_ids: Vec<String>,
    staff_id: Option<String>,
    step: Option<u16>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let path = slots_path(&date, &service_ids, staff_id.as_deref(), step)?;
    get_relay_json::<Value>(&state, &path).await
}

/// Create or update an appointment.
#[tauri::command]
pub async fn crm_write_appointment(
    input: CrmAppointmentWriteInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let (builder, appointment_id) = appointment_builder(input)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(appointment_id.to_string()),
    })
}

fn date(raw: &str) -> Result<chrono::NaiveDate, String> {
    chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| format!("invalid: {raw:?} is not a YYYY-MM-DD date"))
}

fn appointments_path(
    from: Option<&str>,
    to: Option<&str>,
    staff_id: Option<&str>,
    customer_id: Option<&str>,
) -> Result<String, String> {
    let mut params = Vec::new();
    if let Some(from) = from {
        params.push(format!("from={}", date(from)?));
    }
    if let Some(to) = to {
        params.push(format!("to={}", date(to)?));
    }
    if let Some(staff) = staff_id {
        params.push(format!("staff_id={}", parse_uuid(staff, "staff id")?));
    }
    if let Some(customer) = customer_id {
        params.push(format!(
            "customer_id={}",
            parse_uuid(customer, "customer id")?
        ));
    }
    Ok(format!("/crm/appointments?{}", params.join("&")))
}

fn slots_path(
    day: &str,
    service_ids: &[String],
    staff_id: Option<&str>,
    step: Option<u16>,
) -> Result<String, String> {
    let ids = service_ids
        .iter()
        .map(|id| parse_uuid(id, "service id").map(|id| id.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut path = format!(
        "/crm/slots?date={}&service_ids={}",
        date(day)?,
        ids.join(",")
    );
    if let Some(staff) = staff_id {
        path.push_str(&format!("&staff_id={}", parse_uuid(staff, "staff id")?));
    }
    if let Some(step) = step {
        path.push_str(&format!("&step={step}"));
    }
    Ok(path)
}

fn appointment_builder(
    input: CrmAppointmentWriteInput,
) -> Result<(nostr::EventBuilder, uuid::Uuid), String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let appointment_id = parse_uuid(&input.appointment_id, "appointment id")?;
    let start = parse_clock(&input.start).map_err(|e| format!("invalid: start: {e}"))?;
    let payload = AppointmentPayload {
        v: 1,
        staff_id: input.staff_id,
        date: input.date,
        start,
        services: input.services,
        customer_id: input.customer_id,
        client_name: input.client_name,
        client_phone: input.client_phone,
        status: input.status,
        note: input.note,
        source: AppointmentSource::Staff,
        outside_hours: input.outside_hours,
    };
    let builder = buzz_sdk_pkg::build_crm_appointment(
        channel,
        appointment_id,
        payload,
        input.expected_revision.as_deref(),
    )
    .map_err(sdk_error)?;
    Ok((builder, appointment_id))
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

    fn input(start: &str, revision: Option<String>) -> CrmAppointmentWriteInput {
        serde_json::from_value(json!({
            "channel_id": uuid::Uuid::new_v4().to_string(),
            "appointment_id": "6f1c1d9e-3a52-4b8e-9d0e-1f2a3b4c5d6e",
            "expected_revision": revision,
            "staff_id": uuid::Uuid::new_v4(),
            "date": "2026-11-02",
            "start": start,
            "services": [{"product_id": uuid::Uuid::new_v4(), "duration_min": 60, "price_minor": 150000}],
            "customer_id": null,
            "client_name": " Анна ",
            "client_phone": null,
            "status": "booked",
            "note": null,
        }))
        .unwrap()
    }

    #[test]
    fn appointment_carries_id_revision_and_normalized_content() {
        let (builder, id) = appointment_builder(input("10:30", None)).unwrap();
        let event = builder.sign_with_keys(&nostr::Keys::generate()).unwrap();
        assert_eq!(event.kind.as_u16(), 42133);
        assert_eq!(tag(&event, "crm-id"), Some(id.to_string()));
        assert_eq!(tag(&event, "expected-revision"), None);
        let content: Value = serde_json::from_str(&event.content).unwrap();
        assert_eq!(content["start"], "10:30");
        assert_eq!(content["client_name"], "Анна");
        assert_eq!(content["source"], "staff");

        let revision = "ab".repeat(32);
        let (builder, _) = appointment_builder(input("10:30", Some(revision.clone()))).unwrap();
        let event = builder.sign_with_keys(&nostr::Keys::generate()).unwrap();
        assert_eq!(tag(&event, "expected-revision"), Some(revision));
    }

    #[test]
    fn bad_times_are_invalid() {
        for start in ["10:32", "25:00", "1030", "23:30"] {
            let error = appointment_builder(input(start, None)).unwrap_err();
            assert!(error.starts_with("invalid:"), "{start}: {error}");
        }
    }

    #[test]
    fn read_paths_validate_their_inputs() {
        let staff = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            appointments_path(Some("2026-11-02"), Some("2026-11-08"), Some(&staff), None).unwrap(),
            format!("/crm/appointments?from=2026-11-02&to=2026-11-08&staff_id={staff}")
        );
        assert!(appointments_path(Some("2026-11-02&x=1"), None, None, None).is_err());
        let service = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            slots_path("2026-11-02", std::slice::from_ref(&service), None, Some(30)).unwrap(),
            format!("/crm/slots?date=2026-11-02&service_ids={service}&step=30")
        );
        assert!(slots_path("2026-11-02", &["x".into()], None, None).is_err());
    }
}
