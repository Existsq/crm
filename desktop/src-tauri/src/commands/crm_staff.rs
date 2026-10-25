//! CRM staff and working-day commands (relay kinds 42131, 42132,
//! `GET /crm/staff`, `GET /crm/schedule`).
//!
//! Reads return the relay's read model unchanged; writes build the event with
//! `buzz_sdk` (which runs the relay's payload validation), sign it with the
//! active identity and submit it over HTTP.

use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use buzz_crm_pkg::{ScheduleDay, SchedulePayload, StaffPayload};

use super::crm::{parse_uuid, sdk_error, CrmWriteWire};
use crate::{
    app_state::AppState,
    relay::{get_relay_json, submit_event},
};

/// Staff write input. Mirrors `CrmStaffWriteInput` in `tauriCrmStaff.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmStaffWriteInput {
    pub channel_id: String,
    /// Staff id, picked once per dialog for a create so a retry cannot
    /// create a second staff member.
    pub staff_id: String,
    /// Current revision for an update; `None` creates the staff member.
    pub expected_revision: Option<String>,
    pub name: String,
    pub position: Option<String>,
    pub phone: Option<String>,
    #[serde(default)]
    pub service_ids: Vec<uuid::Uuid>,
    pub member: Option<String>,
    #[serde(default)]
    pub archived: bool,
}

/// Working-days input. Mirrors `CrmScheduleInput` in `tauriCrmStaff.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmScheduleInput {
    pub channel_id: String,
    pub staff_id: String,
    /// Days to set, intervals as `HH:MM`; days not listed keep their hours.
    pub days: Vec<ScheduleDay>,
}

/// `GET /crm/staff` as returned by the relay (`{staff}`).
#[tauri::command]
pub async fn crm_list_staff(
    archived: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let path = if archived.unwrap_or(false) {
        "/crm/staff?archived=true"
    } else {
        "/crm/staff"
    };
    get_relay_json::<Value>(&state, path).await
}

/// `GET /crm/schedule?from&to` as returned by the relay (`{from, to, days}`).
#[tauri::command]
pub async fn crm_get_schedule(
    from: String,
    to: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    get_relay_json::<Value>(&state, &schedule_path(&from, &to)?).await
}

/// Create or update a staff member.
#[tauri::command]
pub async fn crm_write_staff(
    input: CrmStaffWriteInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let (builder, staff_id) = staff_builder(input)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(staff_id.to_string()),
    })
}

/// Set working hours for the listed days of one staff member.
#[tauri::command]
pub async fn crm_set_schedule(
    input: CrmScheduleInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let builder = schedule_builder(input)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: None,
    })
}

fn schedule_path(from: &str, to: &str) -> Result<String, String> {
    let date = |raw: &str| {
        chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .map_err(|_| format!("invalid: {raw:?} is not a YYYY-MM-DD date"))
    };
    Ok(format!(
        "/crm/schedule?from={}&to={}",
        date(from)?,
        date(to)?
    ))
}

fn staff_builder(input: CrmStaffWriteInput) -> Result<(nostr::EventBuilder, uuid::Uuid), String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let staff_id = parse_uuid(&input.staff_id, "staff id")?;
    let payload = StaffPayload {
        v: 1,
        name: input.name,
        position: input.position,
        phone: input.phone,
        service_ids: input.service_ids,
        member: input.member,
        archived: input.archived,
    };
    let builder = buzz_sdk_pkg::build_crm_staff(
        channel,
        staff_id,
        payload,
        input.expected_revision.as_deref(),
    )
    .map_err(sdk_error)?;
    Ok((builder, staff_id))
}

fn schedule_builder(input: CrmScheduleInput) -> Result<nostr::EventBuilder, String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let staff_id = parse_uuid(&input.staff_id, "staff id")?;
    buzz_sdk_pkg::build_crm_staff_schedule(
        channel,
        SchedulePayload {
            v: 1,
            staff_id,
            days: input.days,
        },
    )
    .map_err(sdk_error)
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
    fn staff_create_and_update_carry_id_and_revision() {
        let id = uuid::Uuid::new_v4().to_string();
        let input = |revision: Option<String>| -> CrmStaffWriteInput {
            serde_json::from_value(json!({
                "channel_id": uuid::Uuid::new_v4().to_string(),
                "staff_id": id,
                "expected_revision": revision,
                "name": " Olga ",
                "position": null,
                "phone": null,
                "member": null,
            }))
            .unwrap()
        };
        let (builder, built) = staff_builder(input(None)).unwrap();
        let event = builder.sign_with_keys(&nostr::Keys::generate()).unwrap();
        assert_eq!(event.kind.as_u16(), 42131);
        assert_eq!(built.to_string(), id);
        assert_eq!(tag(&event, "crm-id"), Some(id.clone()));
        assert_eq!(tag(&event, "expected-revision"), None);
        let content: Value = serde_json::from_str(&event.content).unwrap();
        assert_eq!(content["name"], "Olga");

        let revision = "ab".repeat(32);
        let (builder, _) = staff_builder(input(Some(revision.clone()))).unwrap();
        let event = builder.sign_with_keys(&nostr::Keys::generate()).unwrap();
        assert_eq!(tag(&event, "expected-revision"), Some(revision));
        assert!(staff_builder(input(Some("nope".into()))).is_err());
    }

    #[test]
    fn schedule_validates_intervals() {
        let input = |end: &str| -> CrmScheduleInput {
            serde_json::from_value(json!({
                "channel_id": uuid::Uuid::new_v4().to_string(),
                "staff_id": uuid::Uuid::new_v4().to_string(),
                "days": [{"date": "2026-11-02", "intervals": [{"start": "09:00", "end": end}]}],
            }))
            .unwrap()
        };
        let event = schedule_builder(input("18:00"))
            .unwrap()
            .sign_with_keys(&nostr::Keys::generate())
            .unwrap();
        assert_eq!(event.kind.as_u16(), 42132);
        assert!(schedule_builder(input("08:00"))
            .unwrap_err()
            .starts_with("invalid:"));
    }

    #[test]
    fn schedule_path_requires_dates() {
        assert_eq!(
            schedule_path("2026-11-02", "2026-11-08").unwrap(),
            "/crm/schedule?from=2026-11-02&to=2026-11-08"
        );
        assert!(schedule_path("2026-11-02&x=1", "2026-11-08").is_err());
    }
}
