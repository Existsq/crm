//! CRM online booking settings commands (relay kind 42135,
//! `GET /crm/online-booking`).
//!
//! The read returns the relay's read model plus the absolute public page URL.
//! The write builds the kind 42135 event here with `serde_json`, because the
//! shared `buzz_sdk` builder (`build_crm_online_booking_settings`) is not in
//! this tree yet; once it lands, `settings_builder` should delegate to it so
//! the desktop runs the relay's own payload validation. Until then the checks
//! below mirror the documented field bounds so obvious mistakes fail locally
//! with the same `invalid:` prefix the relay uses.

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::State;

use super::crm::{parse_uuid, CrmWriteWire};
use crate::{
    app_state::AppState,
    relay::{get_relay_json, relay_api_base_url_with_override, submit_event},
};

/// Relay kind for the online booking settings row (one per CRM space).
const KIND_CRM_ONLINE_BOOKING_SETTINGS: u16 = 42135;
const PAYLOAD_VERSION: u32 = 1;
const TITLE_MAX: usize = 128;
const ABOUT_MAX: usize = 1000;
const ADDRESS_MAX: usize = 256;
const PHONE_MAX: usize = 32;
const HORIZON_DAYS: std::ops::RangeInclusive<u32> = 1..=180;
const MIN_NOTICE_MAX: u32 = 10_080;
const STEP_MIN: std::ops::RangeInclusive<u32> = 5..=60;

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

/// Trimmed text, `None` when blank; errors past `max` characters.
fn optional_text(raw: Option<String>, field: &str, max: usize) -> Result<Option<String>, String> {
    let Some(raw) = raw else { return Ok(None) };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > max {
        return Err(format!("invalid: {field} is longer than {max} characters"));
    }
    Ok(Some(trimmed.to_string()))
}

fn settings_content(input: &CrmOnlineBookingWriteInput) -> Result<String, String> {
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > TITLE_MAX {
        return Err(format!("invalid: title must be 1–{TITLE_MAX} characters"));
    }
    if !HORIZON_DAYS.contains(&input.horizon_days) {
        return Err("invalid: horizon_days must be 1–180".to_string());
    }
    if input.min_notice_min > MIN_NOTICE_MAX {
        return Err(format!(
            "invalid: min_notice_min must be 0–{MIN_NOTICE_MAX}"
        ));
    }
    if !STEP_MIN.contains(&input.step_min) || !input.step_min.is_multiple_of(5) {
        return Err("invalid: step_min must be 5–60 and a multiple of 5".to_string());
    }
    let content = json!({
        "v": PAYLOAD_VERSION,
        "enabled": input.enabled,
        "title": title,
        "about": optional_text(input.about.clone(), "about", ABOUT_MAX)?,
        "address": optional_text(input.address.clone(), "address", ADDRESS_MAX)?,
        "phone": optional_text(input.phone.clone(), "phone", PHONE_MAX)?,
        "horizon_days": input.horizon_days,
        "min_notice_min": input.min_notice_min,
        "step_min": input.step_min,
        "staff_ids": dedup(&input.staff_ids),
        "service_ids": dedup(&input.service_ids),
    });
    serde_json::to_string(&content).map_err(|e| format!("invalid: {e}"))
}

fn dedup(ids: &[uuid::Uuid]) -> Vec<uuid::Uuid> {
    let mut seen = std::collections::HashSet::new();
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

fn tag(parts: &[&str]) -> Result<nostr::Tag, String> {
    nostr::Tag::parse(parts.iter().copied()).map_err(|e| format!("invalid: tag: {e}"))
}

fn settings_builder(input: CrmOnlineBookingWriteInput) -> Result<nostr::EventBuilder, String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let content = settings_content(&input)?;
    let mut tags = vec![tag(&["h", &channel.to_string()])?];
    if let Some(revision) = input.expected_revision.as_deref() {
        nostr::EventId::from_hex(revision)
            .map_err(|_| format!("invalid: expected-revision {revision:?} is not an event id"))?;
        tags.push(tag(&["expected-revision", revision])?);
    }
    Ok(nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_CRM_ONLINE_BOOKING_SETTINGS),
        content,
    )
    .tags(tags))
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
                "about": null,
                "address": "ул. Ленина, 1",
                "phone": null,
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
