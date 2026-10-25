//! CRM visit reminder commands (relay kinds 42150 and 42151,
//! `GET /crm/reminders`, `GET /crm/reminders/deliveries`,
//! `GET /crm/parties/{id}/telegram`).
//!
//! Reads return the relay's read model unchanged; writes build the event with
//! `buzz_sdk` (which runs the relay's payload validation), sign it with the
//! active identity and submit it over HTTP.

use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use buzz_crm_pkg::{ClientTelegramPayload, ReminderRule, ReminderSettingsPayload};

use super::crm::{parse_uuid, sdk_error, CrmWriteWire};
use crate::{
    app_state::AppState,
    relay::{get_relay_json, submit_event},
};

/// Reminder settings write input. Mirrors `CrmReminderSettingsWriteInput`
/// in `tauriCrmReminders.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmReminderSettingsWriteInput {
    pub channel_id: String,
    /// Current revision; `None` for the first save.
    pub expected_revision: Option<String>,
    pub enabled: bool,
    pub rules: Vec<ReminderRule>,
    pub salon_name: Option<String>,
    pub address: Option<String>,
}

/// Telegram link write input. Mirrors `CrmClientTelegramWriteInput`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmClientTelegramWriteInput {
    pub channel_id: String,
    pub customer_id: uuid::Uuid,
    /// `None` unlinks the customer.
    pub chat_id: Option<i64>,
    #[serde(default)]
    pub opted_out: bool,
}

/// `GET /crm/reminders` as returned by the relay.
#[tauri::command]
pub async fn crm_get_reminders(state: State<'_, AppState>) -> Result<Value, String> {
    get_relay_json::<Value>(&state, "/crm/reminders").await
}

/// `GET /crm/reminders/deliveries` as returned by the relay.
#[tauri::command]
pub async fn crm_list_reminder_deliveries(
    since: Option<String>,
    appointment_id: Option<String>,
    customer_id: Option<String>,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let path = deliveries_path(
        since.as_deref(),
        appointment_id.as_deref(),
        customer_id.as_deref(),
        limit,
    )?;
    get_relay_json::<Value>(&state, &path).await
}

/// `GET /crm/parties/{id}/telegram` as returned by the relay.
#[tauri::command]
pub async fn crm_get_client_telegram(
    party_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let id = parse_uuid(&party_id, "party id")?;
    get_relay_json::<Value>(&state, &format!("/crm/parties/{id}/telegram")).await
}

/// Save the space's reminder rules.
#[tauri::command]
pub async fn crm_write_reminder_settings(
    input: CrmReminderSettingsWriteInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let result = submit_event(reminder_settings_builder(input)?, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: None,
    })
}

/// Link, unlink or opt out a customer's Telegram reminders.
#[tauri::command]
pub async fn crm_write_client_telegram(
    input: CrmClientTelegramWriteInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let customer_id = input.customer_id.to_string();
    let result = submit_event(client_telegram_builder(input)?, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(customer_id),
    })
}

fn deliveries_path(
    since: Option<&str>,
    appointment_id: Option<&str>,
    customer_id: Option<&str>,
    limit: Option<u32>,
) -> Result<String, String> {
    let mut params = Vec::new();
    if let Some(since) = since {
        let at = chrono::DateTime::parse_from_rfc3339(since)
            .map_err(|_| format!("invalid: {since:?} is not an RFC 3339 time"))?;
        params.push(format!(
            "since={}",
            at.with_timezone(&chrono::Utc).format("%Y-%m-%dT%H:%M:%SZ")
        ));
    }
    if let Some(id) = appointment_id {
        params.push(format!(
            "appointment_id={}",
            parse_uuid(id, "appointment id")?
        ));
    }
    if let Some(id) = customer_id {
        params.push(format!("customer_id={}", parse_uuid(id, "customer id")?));
    }
    if let Some(limit) = limit {
        params.push(format!("limit={limit}"));
    }
    Ok(if params.is_empty() {
        "/crm/reminders/deliveries".into()
    } else {
        format!("/crm/reminders/deliveries?{}", params.join("&"))
    })
}

fn reminder_settings_builder(
    input: CrmReminderSettingsWriteInput,
) -> Result<nostr::EventBuilder, String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let payload = ReminderSettingsPayload {
        v: 1,
        enabled: input.enabled,
        rules: input.rules,
        salon_name: input.salon_name,
        address: input.address,
    };
    buzz_sdk_pkg::build_crm_reminder_settings(channel, payload, input.expected_revision.as_deref())
        .map_err(sdk_error)
}

fn client_telegram_builder(
    input: CrmClientTelegramWriteInput,
) -> Result<nostr::EventBuilder, String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let payload = ClientTelegramPayload {
        v: 1,
        customer_id: input.customer_id,
        chat_id: input.chat_id,
        opted_out: input.opted_out,
    };
    buzz_sdk_pkg::build_crm_client_telegram(channel, payload).map_err(sdk_error)
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

    fn settings(template: &str, revision: Option<String>) -> CrmReminderSettingsWriteInput {
        serde_json::from_value(json!({
            "channel_id": uuid::Uuid::new_v4().to_string(),
            "expected_revision": revision,
            "enabled": true,
            "rules": [
                {"trigger": "booked", "template": template},
                {"trigger": "before", "offset_min": 1440, "template": "{time}"},
            ],
            "salon_name": " Салон ",
            "address": null,
        }))
        .unwrap()
    }

    #[test]
    fn settings_carry_revision_and_normalized_content() {
        let event = reminder_settings_builder(settings(" {client}, ждём вас ", None))
            .unwrap()
            .sign_with_keys(&nostr::Keys::generate())
            .unwrap();
        assert_eq!(event.kind.as_u16(), 42150);
        assert_eq!(tag(&event, "expected-revision"), None);
        let content: Value = serde_json::from_str(&event.content).unwrap();
        assert_eq!(content["rules"][0]["template"], "{client}, ждём вас");
        assert_eq!(content["salon_name"], "Салон");

        let revision = "ab".repeat(32);
        let event = reminder_settings_builder(settings("{client}", Some(revision.clone())))
            .unwrap()
            .sign_with_keys(&nostr::Keys::generate())
            .unwrap();
        assert_eq!(tag(&event, "expected-revision"), Some(revision));
    }

    #[test]
    fn unknown_placeholders_are_invalid() {
        let error = reminder_settings_builder(settings("{name}", None)).unwrap_err();
        assert!(error.starts_with("invalid:"), "{error}");
    }

    #[test]
    fn telegram_links_build_and_paths_validate() {
        let input: CrmClientTelegramWriteInput = serde_json::from_value(json!({
            "channel_id": uuid::Uuid::new_v4().to_string(),
            "customer_id": uuid::Uuid::new_v4(),
            "chat_id": 123456,
            "opted_out": false,
        }))
        .unwrap();
        let event = client_telegram_builder(input)
            .unwrap()
            .sign_with_keys(&nostr::Keys::generate())
            .unwrap();
        assert_eq!(event.kind.as_u16(), 42151);

        let customer = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            deliveries_path(Some("2026-11-02T10:00:00+03:00"), None, Some(&customer), Some(50))
                .unwrap(),
            format!("/crm/reminders/deliveries?since=2026-11-02T07:00:00Z&customer_id={customer}&limit=50")
        );
        assert_eq!(
            deliveries_path(None, None, None, None).unwrap(),
            "/crm/reminders/deliveries"
        );
        assert!(deliveries_path(Some("yesterday"), None, None, None).is_err());
        assert!(deliveries_path(None, Some("x&y=1"), None, None).is_err());
    }
}
