//! CRM loyalty commands (relay kinds 42170, 42171, `GET /crm/loyalty`).
//!
//! Reads return the relay's read model unchanged; writes build the event with
//! `buzz_sdk` (which runs the relay's payload validation), sign it with the
//! active identity and submit it over HTTP.

use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use buzz_crm_pkg::{BonusAdjustPayload, LoyaltySettingsPayload};

use super::crm::{parse_uuid, sdk_error, CrmWriteWire};
use crate::{
    app_state::AppState,
    relay::{get_relay_json, submit_event},
};

/// Programme input. Mirrors `CrmLoyaltyInput` in `tauriCrmLoyalty.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmLoyaltyInput {
    pub channel_id: String,
    pub enabled: bool,
    pub cashback_bp: u32,
    pub max_redeem_bp: u32,
}

/// Adjustment input. Mirrors `CrmBonusAdjustInput` in `tauriCrmLoyalty.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmBonusAdjustInput {
    pub channel_id: String,
    /// Picked once per dialog so a retried submit cannot apply twice.
    pub adjustment_id: String,
    pub customer_id: String,
    pub delta_minor: i64,
    pub reason: String,
}

/// `GET /crm/loyalty` as returned by the relay.
#[tauri::command]
pub async fn crm_get_loyalty(
    customer_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let path = match customer_id.as_deref() {
        Some(id) => format!(
            "/crm/loyalty?customer_id={}",
            parse_uuid(id, "customer id")?
        ),
        None => "/crm/loyalty".to_string(),
    };
    get_relay_json::<Value>(&state, &path).await
}

/// Replace the bonus programme.
#[tauri::command]
pub async fn crm_set_loyalty(
    input: CrmLoyaltyInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let payload = LoyaltySettingsPayload {
        v: 1,
        enabled: input.enabled,
        cashback_bp: input.cashback_bp,
        max_redeem_bp: input.max_redeem_bp,
    };
    let builder =
        buzz_sdk_pkg::build_crm_loyalty(parse_uuid(&input.channel_id, "channel id")?, payload)
            .map_err(sdk_error)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: None,
    })
}

/// Add or take a customer's bonuses by hand.
#[tauri::command]
pub async fn crm_adjust_bonuses(
    input: CrmBonusAdjustInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let adjustment_id = parse_uuid(&input.adjustment_id, "adjustment id")?;
    let payload = BonusAdjustPayload {
        v: 1,
        customer_id: parse_uuid(&input.customer_id, "customer id")?,
        delta_minor: input.delta_minor,
        reason: input.reason,
    };
    let builder = buzz_sdk_pkg::build_crm_bonus_adjust(
        parse_uuid(&input.channel_id, "channel id")?,
        adjustment_id,
        payload,
    )
    .map_err(sdk_error)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(adjustment_id.to_string()),
    })
}
