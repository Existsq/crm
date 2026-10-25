//! CRM online booking settings commands (relay kind 42135,
//! `GET /crm/online-booking`).
//!
//! The read returns the relay's read model plus the absolute public page URL.
//! The write builds the kind 42135 event with the shared SDK builder, which
//! runs the relay's own payload validation, so bad fields fail locally with
//! the same `invalid:` messages.

use serde::Deserialize;
use serde_json::{json, Value};

use buzz_crm_pkg::OnlineBookingSettings;
use tauri::State;

use super::crm::{parse_uuid, sdk_error, CrmWriteWire};
use crate::{
    app_state::AppState,
    relay::{get_relay_json, relay_api_base_url_with_override, submit_event},
};

/// Online booking settings write input. Mirrors
/// `CrmOnlineBookingWriteInput` in `tauriCrmOnlineBooking.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmOnlineBookingWriteInput {
    pub channel_id: String,
    /// Current revision for an update; `None` creates the settings row.
    pub expected_revision: Option<String>,
    pub enabled: bool,
    pub title: String,
    pub about: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub horizon_days: u32,
    pub min_notice_min: u32,
    pub step_min: u32,
    /// Empty offers every active staff member.
    #[serde(default)]
    pub staff_ids: Vec<uuid::Uuid>,
    /// Empty offers every active service with a duration.
    #[serde(default)]
    pub service_ids: Vec<uuid::Uuid>,
}

/// `GET /crm/online-booking` as returned by the relay
/// (`{settings, revision, public_path}`), plus `public_url`: the relay's HTTP
/// base joined with `public_path`, or `null` when the relay sent no path.
#[tauri::command]
pub async fn crm_get_online_booking(state: State<'_, AppState>) -> Result<Value, String> {
    let mut body = get_relay_json::<Value>(&state, "/crm/online-booking").await?;
    let public_url = public_url(
        &relay_api_base_url_with_override(&state),
        body.get("public_path").and_then(Value::as_str),
    );
    if let Some(object) = body.as_object_mut() {
        object.insert("public_url".to_string(), json!(public_url));
    }
    Ok(body)
}

/// Create or replace the online booking settings.
#[tauri::command]
pub async fn crm_write_online_booking(
    input: CrmOnlineBookingWriteInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let builder = settings_builder(input)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: None,
    })
}

/// The public page URL: `base` without a trailing slash plus a rooted path.
fn public_url(base: &str, public_path: Option<&str>) -> Option<String> {
    let path = public_path?.trim();
    if !path.starts_with('/') || path.starts_with("//") {
        return None;
    }
    Some(format!("{}{}", base.trim_end_matches('/'), path))
}

fn settings_builder(input: CrmOnlineBookingWriteInput) -> Result<nostr::EventBuilder, String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let settings = OnlineBookingSettings {
        v: 0,
        enabled: input.enabled,
        title: input.title,
        about: input.about,
        address: input.address,
        phone: input.phone,
        horizon_days: input.horizon_days,
        min_notice_min: input.min_notice_min,
        step_min: input.step_min,
        staff_ids: input.staff_ids,
        service_ids: input.service_ids,
    };
    buzz_sdk_pkg::build_crm_online_booking_settings(
        channel,
        settings,
        input.expected_revision.as_deref(),
    )
    .map_err(sdk_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag_value(event: &nostr::Event, name: &str) -> Option<String> {
        event
            .tags
            .iter()
            .find(|t| t.as_slice().first().map(String::as_str) == Some(name))
            .and_then(|t| t.as_slice().get(1).cloned())
    }

    fn input(revision: Option<String>) -> CrmOnlineBookingWriteInput {
        serde_json::from_value(json!({
            "channel_id": "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40",
            "expected_revision": revision,
            "enabled": true,
            "title": "  Борода  ",
            "about": "   ",
            "address": "ул. Ленина, 1",
            "phone": null,
            "horizon_days": 30,
            "min_notice_min": 60,
            "step_min": 15,
            "staff_ids": [],
            "service_ids": ["11111111-1111-4111-8111-111111111111", "11111111-1111-4111-8111-111111111111"],
        }))
        .unwrap()
    }

    #[test]
    fn settings_event_carries_channel_revision_and_contract_content() {
        let event = settings_builder(input(None))
            .unwrap()
            .sign_with_keys(&nostr::Keys::generate())
            .unwrap();
        assert_eq!(event.kind.as_u16(), 42135);
        assert_eq!(
            tag_value(&event, "h").as_deref(),
            Some("5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40")
        );
        assert_eq!(tag_value(&event, "expected-revision"), None);
        let content: Value = serde_json::from_str(&event.content).unwrap();
        assert_eq!(
            content,
            json!({
                "v": 1,
                "enabled": true,
                "title": "Борода",
                "address": "ул. Ленина, 1",
                "horizon_days": 30,
                "min_notice_min": 60,
                "step_min": 15,
                "staff_ids": [],
                "service_ids": ["11111111-1111-4111-8111-111111111111"],
            })
        );

        let revision = "ab".repeat(32);
        let event = settings_builder(input(Some(revision.clone())))
            .unwrap()
            .sign_with_keys(&nostr::Keys::generate())
            .unwrap();
        assert_eq!(tag_value(&event, "expected-revision"), Some(revision));
    }

    #[test]
    fn out_of_range_fields_are_invalid() {
        let cases: [(&str, Value); 6] = [
            ("title", json!("  ")),
            ("horizon_days", json!(0)),
            ("horizon_days", json!(181)),
            ("min_notice_min", json!(10_081)),
            ("step_min", json!(12)),
            ("step_min", json!(65)),
        ];
        for (field, value) in cases {
            let mut base = json!({
                "channel_id": "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40",
                "expected_revision": null,
                "enabled": true,
                "title": "Борода",
                "about": null,
                "address": null,
                "phone": null,
                "horizon_days": 30,
                "min_notice_min": 0,
                "step_min": 15,
            });
            base[field] = value;
            let parsed: CrmOnlineBookingWriteInput = serde_json::from_value(base).unwrap();
            let error = settings_builder(parsed).unwrap_err();
            assert!(error.starts_with("invalid:"), "{field}: {error}");
        }
        let error = settings_builder(input(Some("nope".into()))).unwrap_err();
        assert!(error.starts_with("invalid:"), "{error}");
    }

    #[test]
    fn public_url_joins_the_relay_base_and_rooted_path() {
        assert_eq!(
            public_url("https://relay.example/", Some("/book")).as_deref(),
            Some("https://relay.example/book")
        );
        assert_eq!(
            public_url("http://localhost:3000", Some("/book")).as_deref(),
            Some("http://localhost:3000/book")
        );
        assert_eq!(public_url("https://relay.example", None), None);
        assert_eq!(public_url("https://relay.example", Some("book")), None);
        assert_eq!(public_url("https://relay.example", Some("//evil")), None);
    }
}
