//! Loyalty programme: bonus cashback on sales (kinds 42170, 42171).
//!
//! A known customer earns a share of what they paid as bonuses and may spend
//! bonuses on later sales as part of the line discounts (`bonus_minor` on the
//! sale). The relay keeps a per-customer ledger; managers adjust it by hand
//! with a reason (welcome bonus, correction).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{CrmError, Result};
use crate::money::MAX_MONEY_MINOR;
use crate::space::parse_versioned;

/// 100 % in basis points.
pub const MAX_BP: u32 = 10_000;
/// Maximum adjustment reason length in characters.
pub const MAX_REASON_CHARS: usize = 256;

/// Content of a kind 42170 event: the space's loyalty programme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoyaltySettingsPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Whether sales earn and spend bonuses.
    pub enabled: bool,
    /// Bonuses earned, basis points of the amount paid (500 = 5 %).
    pub cashback_bp: u32,
    /// Most of a sale (before bonuses) payable with bonuses, basis points.
    pub max_redeem_bp: u32,
}

impl LoyaltySettingsPayload {
    /// Parse and validate kind 42170 content.
    pub fn parse(content: &str) -> Result<Self> {
        let payload: LoyaltySettingsPayload = parse_versioned(content)?;
        for (value, field) in [
            (payload.cashback_bp, "cashback_bp"),
            (payload.max_redeem_bp, "max_redeem_bp"),
        ] {
            if value > MAX_BP {
                return Err(CrmError::field(field, "must be 0–10000 (0–100 %)"));
            }
        }
        Ok(payload)
    }

    /// Bonuses earned for paying `paid_minor`, rounded down.
    pub fn cashback_minor(&self, paid_minor: i64) -> i64 {
        if !self.enabled {
            return 0;
        }
        share(paid_minor, self.cashback_bp)
    }

    /// Most bonuses spendable on a sale worth `base_minor` before bonuses.
    pub fn max_redeem_minor(&self, base_minor: i64) -> i64 {
        if !self.enabled {
            return 0;
        }
        share(base_minor, self.max_redeem_bp)
    }
}

fn share(amount: i64, bp: u32) -> i64 {
    let value = i128::from(amount.max(0)) * i128::from(bp) / i128::from(MAX_BP);
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Content of a kind 42171 event: a manual change of one customer's bonus
/// balance (the adjustment id is the `crm-id` tag).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BonusAdjustPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Customer whose balance changes.
    pub customer_id: Uuid,
    /// Kopecks of bonuses added (positive) or taken (negative), not zero.
    pub delta_minor: i64,
    /// Why, shown in the customer's bonus history.
    pub reason: String,
}

impl BonusAdjustPayload {
    /// Parse, normalize and validate kind 42171 content.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: BonusAdjustPayload = parse_versioned(content)?;
        if payload.delta_minor == 0 || payload.delta_minor.abs() > MAX_MONEY_MINOR {
            return Err(CrmError::field(
                "delta_minor",
                format!("must be non-zero and at most {MAX_MONEY_MINOR} kopecks either way"),
            ));
        }
        let reason = payload.reason.trim();
        if reason.is_empty()
            || reason.chars().count() > MAX_REASON_CHARS
            || reason.chars().any(char::is_control)
        {
            return Err(CrmError::field(
                "reason",
                format!("must be 1–{MAX_REASON_CHARS} printable characters"),
            ));
        }
        payload.reason = reason.to_owned();
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_bound_percents_and_compute_shares() {
        let settings = LoyaltySettingsPayload::parse(
            r#"{"v":1,"enabled":true,"cashback_bp":500,"max_redeem_bp":3000}"#,
        )
        .unwrap();
        assert_eq!(settings.cashback_minor(199_999), 9_999);
        assert_eq!(settings.max_redeem_minor(100_000), 30_000);
        let off = LoyaltySettingsPayload {
            enabled: false,
            ..settings
        };
        assert_eq!(off.cashback_minor(100_000), 0);
        assert_eq!(off.max_redeem_minor(100_000), 0);
        assert!(LoyaltySettingsPayload::parse(
            r#"{"v":1,"enabled":true,"cashback_bp":10001,"max_redeem_bp":0}"#
        )
        .is_err());
    }

    #[test]
    fn adjustments_need_a_delta_and_a_reason() {
        let id = Uuid::from_u128(3);
        let ok = BonusAdjustPayload::parse(&format!(
            r#"{{"v":1,"customer_id":"{id}","delta_minor":-5000,"reason":"  списание "}}"#
        ))
        .unwrap();
        assert_eq!(ok.reason, "списание");
        for bad in [
            r#""delta_minor":0,"reason":"x""#,
            r#""delta_minor":5,"reason":"  ""#,
            r#""delta_minor":100000000001,"reason":"x""#,
        ] {
            let content = format!(r#"{{"v":1,"customer_id":"{id}",{bad}}}"#);
            assert!(BonusAdjustPayload::parse(&content).is_err(), "{bad}");
        }
    }
}
