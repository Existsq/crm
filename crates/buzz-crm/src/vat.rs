//! VAT rates (НДС) as a product attribute.
//!
//! Rates follow the Russian tax code as of 2026: the general rate is 22 %
//! (20 % remains only for historical documents), 10 % is the reduced rate,
//! 5 % and 7 % apply to simplified-taxation (УСН) payers, and `vatNNN` values
//! are the calculated rates (`22/122`, …) used for prepayments. The ATOL tax
//! tag is derived from these names in `buzz-atol`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::CrmError;

/// A VAT rate attached to a product or document line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VatRate {
    /// Not subject to VAT (без НДС).
    None,
    /// 0 %.
    Vat0,
    /// 5 % (УСН).
    Vat5,
    /// 7 % (УСН).
    Vat7,
    /// 10 %.
    Vat10,
    /// 20 % — superseded by 22 % from 2026-01-01; kept for historical data.
    Vat20,
    /// 22 %.
    Vat22,
    /// Calculated 5/105.
    Vat105,
    /// Calculated 7/107.
    Vat107,
    /// Calculated 10/110.
    Vat110,
    /// Calculated 20/120 — historical.
    Vat120,
    /// Calculated 22/122.
    Vat122,
}

impl VatRate {
    /// Every rate, in display order.
    pub const ALL: [VatRate; 12] = [
        VatRate::None,
        VatRate::Vat0,
        VatRate::Vat5,
        VatRate::Vat7,
        VatRate::Vat10,
        VatRate::Vat20,
        VatRate::Vat22,
        VatRate::Vat105,
        VatRate::Vat107,
        VatRate::Vat110,
        VatRate::Vat120,
        VatRate::Vat122,
    ];

    /// Canonical wire name (`"vat22"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            VatRate::None => "none",
            VatRate::Vat0 => "vat0",
            VatRate::Vat5 => "vat5",
            VatRate::Vat7 => "vat7",
            VatRate::Vat10 => "vat10",
            VatRate::Vat20 => "vat20",
            VatRate::Vat22 => "vat22",
            VatRate::Vat105 => "vat105",
            VatRate::Vat107 => "vat107",
            VatRate::Vat110 => "vat110",
            VatRate::Vat120 => "vat120",
            VatRate::Vat122 => "vat122",
        }
    }

    /// True for rates that must not be chosen for new products (20 %, 20/120).
    pub const fn is_historical(self) -> bool {
        matches!(self, VatRate::Vat20 | VatRate::Vat120)
    }
}

impl fmt::Display for VatRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for VatRate {
    type Err = CrmError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        VatRate::ALL
            .into_iter()
            .find(|rate| rate.as_str() == s)
            .ok_or_else(|| CrmError::field("vat", format!("unknown VAT rate {s:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_through_str_and_serde() {
        for rate in VatRate::ALL {
            assert_eq!(rate.as_str().parse::<VatRate>().unwrap(), rate);
            let json = serde_json::to_string(&rate).unwrap();
            assert_eq!(json, format!("\"{}\"", rate.as_str()));
            assert_eq!(serde_json::from_str::<VatRate>(&json).unwrap(), rate);
        }
    }

    #[test]
    fn only_twenty_percent_rates_are_historical() {
        let historical: Vec<_> = VatRate::ALL
            .into_iter()
            .filter(|r| r.is_historical())
            .collect();
        assert_eq!(historical, vec![VatRate::Vat20, VatRate::Vat120]);
    }

    #[test]
    fn unknown_rate_is_rejected() {
        assert!("vat18".parse::<VatRate>().is_err());
        assert!("VAT22".parse::<VatRate>().is_err());
    }
}
