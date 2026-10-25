//! Fiscal receipts (kind 42120) and cash-register shifts (kind 42121).
//!
//! The desktop prints a receipt on the ККМ (ADR-0003) and then records what
//! the device returned as a signed event, so the sale shows "чек пробит" and
//! the fiscal sign can be found later. The relay never talks to the device.

use serde::{Deserialize, Serialize};

use crate::error::{CrmError, Result};
use crate::money::Money;
use crate::space::parse_versioned;

/// Maximum length of a device-issued identifier (FN number, fiscal sign…).
pub const MAX_FISCAL_ID_CHARS: usize = 32;

/// What a fiscal receipt records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FiscalOperation {
    /// Sale receipt (приход).
    Sell,
    /// Return receipt for a voided sale (возврат прихода).
    SellReturn,
}

impl FiscalOperation {
    /// Canonical wire name (`"sell"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            FiscalOperation::Sell => "sell",
            FiscalOperation::SellReturn => "sell_return",
        }
    }
}

/// Fiscal attributes the ККМ returns for a printed document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FiscalParams {
    /// Fiscal document number (ФД).
    pub document_number: u64,
    /// Fiscal sign (ФП/ФПД), digits.
    pub document_sign: String,
    /// Receipt number within the shift.
    pub receipt_number: u64,
    /// Shift number.
    pub shift_number: u64,
    /// Fiscal storage number (ФН).
    pub fn_number: String,
    /// Registration number of the ККТ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration_number: Option<String>,
    /// Document date and time as printed (device local time, ISO 8601).
    pub document_datetime: String,
}

impl FiscalParams {
    fn validate(&self) -> Result<()> {
        if self.document_number == 0 || self.shift_number == 0 {
            return Err(CrmError::field(
                "fiscal",
                "document and shift numbers start at 1",
            ));
        }
        device_id(&self.document_sign, "fiscal.document_sign", true)?;
        device_id(&self.fn_number, "fiscal.fn_number", true)?;
        if let Some(number) = &self.registration_number {
            device_id(number, "fiscal.registration_number", true)?;
        }
        let datetime = self.document_datetime.trim();
        if datetime.is_empty() || datetime.len() > 40 {
            return Err(CrmError::field(
                "fiscal.document_datetime",
                "must be a date and time",
            ));
        }
        Ok(())
    }
}

/// Content of a kind 42120 event; the `crm-id` tag is the sale id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FiscalReceiptPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Sale or return receipt.
    pub operation: FiscalOperation,
    /// Receipt total as printed, kopecks; must equal the sale total.
    pub total_minor: Money,
    /// UUID of the device task that printed it (`UUIDv5("sell:" + sale)`).
    pub task_uuid: String,
    /// What the device returned.
    pub fiscal: FiscalParams,
}

impl FiscalReceiptPayload {
    /// Parse and validate kind 42120 content.
    pub fn parse(content: &str) -> Result<Self> {
        let payload: FiscalReceiptPayload = parse_versioned(content)?;
        Money::non_negative(payload.total_minor.minor(), "total_minor")?;
        uuid::Uuid::parse_str(&payload.task_uuid)
            .map_err(|_| CrmError::field("task_uuid", "must be a UUID"))?;
        payload.fiscal.validate()?;
        Ok(payload)
    }
}

/// What happened to the shift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShiftAction {
    /// The shift was opened on the device.
    Open,
    /// The shift was closed (Z-report printed).
    Close,
}

impl ShiftAction {
    /// Canonical wire name (`"open"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            ShiftAction::Open => "open",
            ShiftAction::Close => "close",
        }
    }
}

/// Content of a kind 42121 event recording a shift opened or closed on a ККМ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CashShiftPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Open or close.
    pub action: ShiftAction,
    /// Fiscal storage (ФН) the shift belongs to.
    pub fn_number: String,
    /// Shift number on that ФН.
    pub shift_number: u64,
    /// Fiscal document number of the open/close report.
    pub document_number: u64,
    /// Device date and time of the report (ISO 8601).
    pub document_datetime: String,
}

impl CashShiftPayload {
    /// Parse and validate kind 42121 content.
    pub fn parse(content: &str) -> Result<Self> {
        let payload: CashShiftPayload = parse_versioned(content)?;
        device_id(&payload.fn_number, "fn_number", true)?;
        if payload.shift_number == 0 || payload.document_number == 0 {
            return Err(CrmError::field(
                "shift_number",
                "shift and document numbers start at 1",
            ));
        }
        if payload.document_datetime.trim().is_empty() || payload.document_datetime.len() > 40 {
            return Err(CrmError::field(
                "document_datetime",
                "must be a date and time",
            ));
        }
        Ok(payload)
    }
}

fn device_id(value: &str, field: &'static str, digits_only: bool) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= MAX_FISCAL_ID_CHARS
        && value.chars().all(|c| {
            if digits_only {
                c.is_ascii_digit()
            } else {
                c.is_ascii_alphanumeric()
            }
        });
    if valid {
        Ok(())
    } else {
        Err(CrmError::field(
            field,
            format!("must be 1–{MAX_FISCAL_ID_CHARS} digits"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn receipt() -> serde_json::Value {
        json!({
            "v": 1,
            "operation": "sell",
            "total_minor": 91_988,
            "task_uuid": "5f0c6e0e-6d1a-5b2e-9c3d-1a2b3c4d5e6f",
            "fiscal": {
                "document_number": 17,
                "document_sign": "3826380862",
                "receipt_number": 4,
                "shift_number": 2,
                "fn_number": "9999078900004792",
                "registration_number": "0000000001002292",
                "document_datetime": "2026-10-25T14:03:00+03:00"
            }
        })
    }

    #[test]
    fn valid_receipt_parses() {
        let payload = FiscalReceiptPayload::parse(&receipt().to_string()).unwrap();
        assert_eq!(payload.operation, FiscalOperation::Sell);
        assert_eq!(payload.fiscal.document_number, 17);
    }

    #[test]
    fn invalid_receipts_are_rejected() {
        let cases = [
            ("/task_uuid", json!("nope")),
            ("/total_minor", json!(-1)),
            ("/operation", json!("refund")),
            ("/fiscal/document_number", json!(0)),
            ("/fiscal/document_sign", json!("38a")),
            ("/fiscal/fn_number", json!("")),
        ];
        for (pointer, value) in cases {
            let mut payload = receipt();
            *payload.pointer_mut(pointer).unwrap() = value.clone();
            assert!(
                FiscalReceiptPayload::parse(&payload.to_string()).is_err(),
                "{pointer} = {value}"
            );
        }
    }

    #[test]
    fn shifts_validate_numbers() {
        let shift = json!({
            "v": 1, "action": "open", "fn_number": "9999078900004792",
            "shift_number": 3, "document_number": 21,
            "document_datetime": "2026-10-25T09:00:00+03:00"
        });
        assert_eq!(
            CashShiftPayload::parse(&shift.to_string()).unwrap().action,
            ShiftAction::Open
        );
        let mut bad = shift.clone();
        bad["shift_number"] = json!(0);
        assert!(CashShiftPayload::parse(&bad.to_string()).is_err());
    }
}
