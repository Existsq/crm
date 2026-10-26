//! Staff pay rates and payouts (kinds 42160, 42161, 42162).
//!
//! A staff member earns a percent of the services they performed, a fixed
//! amount per performed visit and a fixed amount per scheduled working day.
//! The relay computes what is accrued for a period from the journal; payouts
//! record what was actually handed over, so the balance is accrued − paid.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{CrmError, Result};
use crate::money::MAX_MONEY_MINOR;
use crate::space::parse_versioned;
use crate::staff::check_date;

/// 100 % in basis points.
pub const MAX_PERCENT_BP: u32 = 10_000;
/// Maximum payout note length in characters.
pub const MAX_PAYOUT_NOTE_CHARS: usize = 256;

/// Content of a kind 42160 event: the current pay rates of one staff member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayRatesPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Staff member these rates apply to.
    pub staff_id: Uuid,
    /// Share of performed services, basis points (1500 = 15 %).
    #[serde(default)]
    pub percent_bp: u32,
    /// Fixed amount per performed visit, kopecks.
    #[serde(default)]
    pub per_visit_minor: i64,
    /// Fixed amount per scheduled working day, kopecks.
    #[serde(default)]
    pub shift_minor: i64,
}

impl PayRatesPayload {
    /// Parse and validate kind 42160 content.
    pub fn parse(content: &str) -> Result<Self> {
        let payload: PayRatesPayload = parse_versioned(content)?;
        if payload.percent_bp > MAX_PERCENT_BP {
            return Err(CrmError::field("percent_bp", "must be 0–10000 (0–100 %)"));
        }
        check_amount(payload.per_visit_minor, "per_visit_minor", true)?;
        check_amount(payload.shift_minor, "shift_minor", true)?;
        Ok(payload)
    }
}

/// Content of a kind 42161 event: money paid to a staff member (the payout id
/// is the `crm-id` tag).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayoutPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Staff member paid.
    pub staff_id: Uuid,
    /// Amount paid, kopecks (positive).
    pub amount_minor: i64,
    /// Calendar date `YYYY-MM-DD` of the payout in the space timezone.
    pub date: String,
    /// Free-form note, e.g. «аванс».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl PayoutPayload {
    /// Parse, normalize and validate kind 42161 content.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: PayoutPayload = parse_versioned(content)?;
        check_amount(payload.amount_minor, "amount_minor", false)?;
        check_date(&payload.date)?;
        payload.note = payload
            .note
            .as_deref()
            .map(str::trim)
            .filter(|note| !note.is_empty())
            .map(|note| {
                if note.chars().count() > MAX_PAYOUT_NOTE_CHARS
                    || note.chars().any(char::is_control)
                {
                    Err(CrmError::field(
                        "note",
                        format!("must be at most {MAX_PAYOUT_NOTE_CHARS} printable characters"),
                    ))
                } else {
                    Ok(note.to_owned())
                }
            })
            .transpose()?;
        Ok(payload)
    }
}

/// Content of a kind 42162 event: cancel a payout recorded by mistake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayoutVoidPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// The payout to cancel.
    pub payout_id: Uuid,
}

impl PayoutVoidPayload {
    /// Parse and validate kind 42162 content.
    pub fn parse(content: &str) -> Result<Self> {
        parse_versioned(content)
    }
}

fn check_amount(value: i64, field: &'static str, allow_zero: bool) -> Result<()> {
    let low = if allow_zero { 0 } else { 1 };
    if (low..=MAX_MONEY_MINOR).contains(&value) {
        Ok(())
    } else {
        Err(CrmError::field(
            field,
            format!("must be {low}–{MAX_MONEY_MINOR} kopecks"),
        ))
    }
}

/// What a staff member earned: `revenue × percent + visits × per_visit +
/// shifts × shift`, the percent part rounded down to whole kopecks.
pub fn accrued_minor(rates: &PayRatesPayload, revenue_minor: i64, visits: i64, shifts: i64) -> i64 {
    let percent =
        i128::from(revenue_minor) * i128::from(rates.percent_bp) / i128::from(MAX_PERCENT_BP);
    let fixed = i128::from(visits) * i128::from(rates.per_visit_minor)
        + i128::from(shifts) * i128::from(rates.shift_minor);
    i64::try_from(percent + fixed).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staff() -> Uuid {
        Uuid::from_u128(7)
    }

    #[test]
    fn rates_parse_with_defaults_and_bounds() {
        let rates = PayRatesPayload::parse(&format!(
            r#"{{"v":1,"staff_id":"{}","percent_bp":4000}}"#,
            staff()
        ))
        .unwrap();
        assert_eq!(rates.percent_bp, 4000);
        assert_eq!(rates.per_visit_minor, 0);
        assert_eq!(rates.shift_minor, 0);
        for bad in [
            r#""percent_bp":10001"#,
            r#""per_visit_minor":-1"#,
            r#""shift_minor":100000000001"#,
        ] {
            let content = format!(r#"{{"v":1,"staff_id":"{}",{bad}}}"#, staff());
            assert!(PayRatesPayload::parse(&content).is_err(), "{bad}");
        }
    }

    #[test]
    fn payouts_need_a_positive_amount_and_a_date() {
        let ok = PayoutPayload::parse(&format!(
            r#"{{"v":1,"staff_id":"{}","amount_minor":500000,"date":"2026-10-25","note":"  аванс "}}"#,
            staff()
        ))
        .unwrap();
        assert_eq!(ok.note.as_deref(), Some("аванс"));
        let blank = PayoutPayload::parse(&format!(
            r#"{{"v":1,"staff_id":"{}","amount_minor":1,"date":"2026-10-25","note":"  "}}"#,
            staff()
        ))
        .unwrap();
        assert_eq!(blank.note, None);
        for bad in [
            r#""amount_minor":0,"date":"2026-10-25""#,
            r#""amount_minor":100,"date":"2026-13-01""#,
            r#""amount_minor":100"#,
        ] {
            let content = format!(r#"{{"v":1,"staff_id":"{}",{bad}}}"#, staff());
            assert!(PayoutPayload::parse(&content).is_err(), "{bad}");
        }
    }

    #[test]
    fn accrual_combines_percent_visits_and_shifts() {
        let rates = PayRatesPayload {
            v: 1,
            staff_id: staff(),
            percent_bp: 3333,
            per_visit_minor: 10_000,
            shift_minor: 50_000,
        };
        // 33.33 % of 1 000,01 ₽ = 333,30 ₽ rounded down; 2 × 100 ₽; 3 × 500 ₽.
        assert_eq!(
            accrued_minor(&rates, 100_001, 2, 3),
            33_330 + 20_000 + 150_000
        );
        assert_eq!(accrued_minor(&rates, 0, 0, 0), 0);
    }
}
