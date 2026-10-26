//! CRM payroll commands (relay kinds 42160–42162, `GET /crm/payroll`).
//!
//! Reads return the relay's read model unchanged; writes build the event with
//! `buzz_sdk` (which runs the relay's payload validation), sign it with the
//! active identity and submit it over HTTP.

use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use buzz_crm_pkg::{PayRatesPayload, PayoutPayload};

use super::crm::{parse_uuid, sdk_error, CrmWriteWire};
use crate::{
    app_state::AppState,
    relay::{get_relay_json, submit_event},
};

/// Pay rates input. Mirrors `CrmPayRatesInput` in `tauriCrmPayroll.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmPayRatesInput {
    pub channel_id: String,
    pub staff_id: String,
    pub percent_bp: u32,
    pub per_visit_minor: i64,
    pub shift_minor: i64,
}

/// Payout input. Mirrors `CrmPayoutInput` in `tauriCrmPayroll.ts`.
#[derive(Debug, Clone, Deserialize)]
pub struct CrmPayoutInput {
    pub channel_id: String,
    /// Picked once per dialog so a retried submit cannot pay twice.
    pub payout_id: String,
    pub staff_id: String,
    pub amount_minor: i64,
    pub date: String,
    pub note: Option<String>,
}

/// `GET /crm/payroll` as returned by the relay.
#[tauri::command]
pub async fn crm_get_payroll(
    from: Option<String>,
    to: Option<String>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    get_relay_json::<Value>(&state, &payroll_path(from.as_deref(), to.as_deref())?).await
}

/// Replace a staff member's pay rates.
#[tauri::command]
pub async fn crm_set_pay_rates(
    input: CrmPayRatesInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let payload = PayRatesPayload {
        v: 1,
        staff_id: parse_uuid(&input.staff_id, "staff id")?,
        percent_bp: input.percent_bp,
        per_visit_minor: input.per_visit_minor,
        shift_minor: input.shift_minor,
    };
    let builder = buzz_sdk_pkg::build_crm_pay_rates(channel, payload).map_err(sdk_error)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: None,
    })
}

/// Record a payout.
#[tauri::command]
pub async fn crm_record_payout(
    input: CrmPayoutInput,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let channel = parse_uuid(&input.channel_id, "channel id")?;
    let payout_id = parse_uuid(&input.payout_id, "payout id")?;
    let payload = PayoutPayload {
        v: 1,
        staff_id: parse_uuid(&input.staff_id, "staff id")?,
        amount_minor: input.amount_minor,
        date: input.date,
        note: input.note,
    };
    let builder = buzz_sdk_pkg::build_crm_payout(channel, payout_id, payload).map_err(sdk_error)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: Some(payout_id.to_string()),
    })
}

/// Cancel a payout.
#[tauri::command]
pub async fn crm_void_payout(
    channel_id: String,
    payout_id: String,
    state: State<'_, AppState>,
) -> Result<CrmWriteWire, String> {
    let builder = buzz_sdk_pkg::build_crm_payout_void(
        parse_uuid(&channel_id, "channel id")?,
        parse_uuid(&payout_id, "payout id")?,
    )
    .map_err(sdk_error)?;
    let result = submit_event(builder, &state).await?;
    Ok(CrmWriteWire {
        event_id: result.event_id,
        entity_id: None,
    })
}

fn payroll_path(from: Option<&str>, to: Option<&str>) -> Result<String, String> {
    let mut params = Vec::new();
    for (key, value) in [("from", from), ("to", to)] {
        if let Some(day) = value {
            let day = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
                .map_err(|_| format!("invalid: {day:?} is not a YYYY-MM-DD date"))?;
            params.push(format!("{key}={day}"));
        }
    }
    Ok(format!("/crm/payroll?{}", params.join("&")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payroll_path_validates_days() {
        assert_eq!(
            payroll_path(Some("2026-10-01"), Some("2026-10-31")).unwrap(),
            "/crm/payroll?from=2026-10-01&to=2026-10-31"
        );
        assert_eq!(payroll_path(None, None).unwrap(), "/crm/payroll?");
        assert!(payroll_path(Some("01.10.2026"), None).is_err());
    }
}
