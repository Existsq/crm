//! CRM space and warehouse commands (relay kinds 42100–42101, `GET /crm/space`).
//!
//! Reads return the relay's read model unchanged; writes build events with the
//! shared `buzz-sdk` CRM builders (which run the same payload validation as the
//! relay), sign them with the active identity and submit them over HTTP.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use buzz_crm_pkg::{SpaceSettings, WarehouseSeed};

use crate::{
    app_state::AppState,
    relay::{get_relay_json, submit_event},
};

/// Result of a CRM write. Mirrors `RawCrmWriteResult` in `tauriCrm.ts`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CrmWriteWire {
    pub event_id: String,
    /// Entity created or updated by the write, when the command owns its id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
}

/// Warehouse write input. Mirrors `CrmWarehouseWriteInput` in `tauriCrm.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmWarehouseWriteInput {
    pub channel_id: String,
    /// `None` creates a new warehouse with a fresh id.
    pub warehouse_id: Option<String>,
    pub name: String,
    pub is_default: bool,
    pub archived: bool,
    /// Required when `warehouse_id` is set.
    pub expected_revision: Option<String>,
}

fn parse_uuid(raw: &str, what: &str) -> Result<uuid::Uuid, String> {
    uuid::Uuid::parse_str(raw).map_err(|_| format!("invalid {what}: {raw}"))
}

fn sdk_error(error: buzz_sdk_pkg::SdkError) -> String {
    format!("invalid: {error}")
}

/// `GET /crm/space` as returned by the relay.
#[tauri::command]
pub async fn crm_get_space(state: State<'_, AppState>) -> Result<Value, String> {
    get_relay_json::<Value>(&state, "/crm/space").await
}

/// Bind `channel_id` as the community's CRM space with a default warehouse.
#[tauri::command]
pub async fn crm_create_space(
    channel_id: String,
    settings: SpaceSettings,
    warehouse_name: String,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let channel = parse_uuid(&channel_id, "channel id")?;
    let warehouse_id = uuid::Uuid::new_v4();
    let builder = buzz_sdk_pkg::build_crm_space_create(
        channel,
        settings,
        WarehouseSeed {
            id: warehouse_id,
            name: warehouse_name,
        },
    )
    .map_err(sdk_error)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(warehouse_id.to_string()),
    })
}

/// Replace the space settings, guarded by the current revision.
#[tauri::command]
pub async fn crm_update_space(
    channel_id: String,
    settings: SpaceSettings,
    expected_revision: String,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let channel = parse_uuid(&channel_id, "channel id")?;
    let builder = buzz_sdk_pkg::build_crm_space_update(channel, settings, &expected_revision)
        .map_err(sdk_error)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: None,
    })
}

/// Create or update a warehouse.
#[tauri::command]
pub async fn crm_write_warehouse(
    input: CrmWarehouseWriteInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let (builder, warehouse_id) = warehouse_builder(&input)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(warehouse_id.to_string()),
    })
}

fn warehouse_builder(
    input: &CrmWarehouseWriteInput,
) -> Result<(nostr::EventBuilder, uuid::Uuid), String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let (warehouse_id, expected_revision) =
        match (&input.warehouse_id, input.expected_revision.as_deref()) {
            (None, None) => (uuid::Uuid::new_v4(), None),
            (Some(id), Some(revision)) => (parse_uuid(id, "warehouse id")?, Some(revision)),
            (None, Some(_)) => {
                return Err("invalid: a new warehouse cannot carry a revision".to_string())
            }
            (Some(_), None) => {
                return Err("invalid: updating a warehouse needs its current revision".to_string())
            }
        };
    let builder = buzz_sdk_pkg::build_crm_warehouse(
        channel,
        warehouse_id,
        &input.name,
        input.is_default,
        input.archived,
        expected_revision,
    )
    .map_err(sdk_error)?;
    Ok((builder, warehouse_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(warehouse_id: Option<&str>, revision: Option<&str>) -> CrmWarehouseWriteInput {
        CrmWarehouseWriteInput {
            channel_id: uuid::Uuid::new_v4().to_string(),
            warehouse_id: warehouse_id.map(str::to_string),
            name: "Склад".to_string(),
            is_default: false,
            archived: false,
            expected_revision: revision.map(str::to_string),
        }
    }

    fn tag(event: &nostr::Event, name: &str) -> Option<String> {
        event
            .tags
            .iter()
            .find(|t| t.as_slice().first().map(String::as_str) == Some(name))
            .and_then(|t| t.as_slice().get(1).cloned())
    }

    #[test]
    fn create_gets_a_fresh_id_and_no_revision() {
        let (builder, id) = warehouse_builder(&input(None, None)).unwrap();
        let event = builder.sign_with_keys(&nostr::Keys::generate()).unwrap();
        assert_eq!(tag(&event, "crm-id"), Some(id.to_string()));
        assert_eq!(tag(&event, "expected-revision"), None);
    }

    #[test]
    fn update_keeps_id_and_carries_revision() {
        let id = uuid::Uuid::new_v4().to_string();
        let revision = "ab".repeat(32);
        let (builder, built_id) = warehouse_builder(&input(Some(&id), Some(&revision))).unwrap();
        let event = builder.sign_with_keys(&nostr::Keys::generate()).unwrap();
        assert_eq!(built_id.to_string(), id);
        assert_eq!(tag(&event, "expected-revision"), Some(revision));
    }

    #[test]
    fn mismatched_id_and_revision_are_rejected() {
        assert!(warehouse_builder(&input(None, Some(&"ab".repeat(32)))).is_err());
        assert!(warehouse_builder(&input(Some(&uuid::Uuid::new_v4().to_string()), None)).is_err());
        assert!(warehouse_builder(&input(Some("not-a-uuid"), Some(&"ab".repeat(32)))).is_err());
    }
}
