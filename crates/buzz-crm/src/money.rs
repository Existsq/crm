//! Integer money and quantity (AD-004).
//!
//! Money is stored in kopecks, quantities in thousandths of a unit, so every
//! participant (relay, SDK, CLI, desktop) rounds identically. Line amounts are
//! computed in `i128` and rounded half-up once per line; the same rule is
//! replayed by the desktop from `fixtures/money_vectors.json`.

use serde::{Deserialize, Serialize};

use crate::error::{CrmError, Result};

/// Largest accepted money value: 1 000 000 000.00 ₽ in kopecks.
pub const MAX_MONEY_MINOR: i64 = 100_000_000_000;
/// Largest accepted quantity: 1 000 000 units in thousandths.
pub const MAX_QTY_MILLI: i64 = 1_000_000_000;
/// Thousandths per whole unit.
pub const QTY_SCALE: i64 = 1_000;

/// An amount of money in kopecks (1/100 RUB).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Money(i64);

impl Money {
    /// Zero kopecks.
    pub const ZERO: Money = Money(0);

    /// Build a money value, rejecting anything outside `±MAX_MONEY_MINOR`.
    pub fn from_minor(minor: i64) -> Result<Self> {
        if minor.unsigned_abs() > MAX_MONEY_MINOR as u64 {
            return Err(CrmError::Overflow);
        }
        Ok(Self(minor))
    }

    /// Build a non-negative money value (prices, payments).
    pub fn non_negative(minor: i64, field: &'static str) -> Result<Self> {
        if minor < 0 {
            return Err(CrmError::field(field, "must not be negative"));
        }
        Self::from_minor(minor).map_err(|_| CrmError::field(field, "out of range"))
    }

    /// The value in kopecks.
    pub const fn minor(self) -> i64 {
        self.0
    }

    /// Checked addition within the domain bounds.
    pub fn checked_add(self, other: Money) -> Result<Money> {
        Money::from_minor(self.0.checked_add(other.0).ok_or(CrmError::Overflow)?)
    }

    /// Checked subtraction within the domain bounds.
    pub fn checked_sub(self, other: Money) -> Result<Money> {
        Money::from_minor(self.0.checked_sub(other.0).ok_or(CrmError::Overflow)?)
    }

    /// Sum an iterator of money values with bounds checking.
    pub fn checked_sum<I: IntoIterator<Item = Money>>(values: I) -> Result<Money> {
        values
            .into_iter()
            .try_fold(Money::ZERO, |acc, value| acc.checked_add(value))
    }
}

/// A quantity in thousandths of a unit (0.001 kg, 0.001 pcs).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Qty(i64);

impl Qty {
    /// Zero quantity.
    pub const ZERO: Qty = Qty(0);

    /// Build a quantity, rejecting anything outside `±MAX_QTY_MILLI`.
    pub fn from_milli(milli: i64) -> Result<Self> {
        if milli.unsigned_abs() > MAX_QTY_MILLI as u64 {
            return Err(CrmError::Overflow);
        }
        Ok(Self(milli))
    }

    /// Build a strictly positive quantity (document lines).
    pub fn positive(milli: i64, field: &'static str) -> Result<Self> {
        if milli <= 0 {
            return Err(CrmError::field(field, "must be greater than zero"));
        }
        Self::from_milli(milli).map_err(|_| CrmError::field(field, "out of range"))
    }

    /// Whole units, i.e. `units * 1000` thousandths.
    pub fn from_units(units: i64) -> Result<Self> {
        Self::from_milli(units.checked_mul(QTY_SCALE).ok_or(CrmError::Overflow)?)
    }

    /// The value in thousandths.
    pub const fn milli(self) -> i64 {
        self.0
    }

    /// True when the quantity is a whole number of units.
    pub const fn is_whole(self) -> bool {
        self.0 % QTY_SCALE == 0
    }
}

/// `price × qty`, rounded half-up to whole kopecks.
///
/// `price` is per whole unit, `qty` is in thousandths. Negative inputs round
/// half away from zero so that a negated line is exactly the negated amount.
pub fn line_amount(price: Money, qty: Qty) -> Result<Money> {
    let raw = i128::from(price.minor()) * i128::from(qty.milli());
    let scale = i128::from(QTY_SCALE);
    let half = scale / 2;
    let rounded = if raw >= 0 {
        (raw + half) / scale
    } else {
        -((-raw + half) / scale)
    };
    let minor = i64::try_from(rounded).map_err(|_| CrmError::Overflow)?;
    Money::from_minor(minor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[derive(serde::Deserialize)]
    struct Vector {
        price_minor: i64,
        qty_milli: i64,
        amount_minor: i64,
    }

    #[test]
    fn golden_vectors_match() {
        let vectors: Vec<Vector> =
            serde_json::from_str(include_str!("../fixtures/money_vectors.json"))
                .expect("fixture parses");
        assert!(vectors.len() >= 10);
        for v in vectors {
            let got = line_amount(
                Money::from_minor(v.price_minor).unwrap(),
                Qty::from_milli(v.qty_milli).unwrap(),
            )
            .unwrap();
            assert_eq!(
                got.minor(),
                v.amount_minor,
                "price {} × qty {}",
                v.price_minor,
                v.qty_milli
            );
        }
    }

    #[test]
    fn half_rounds_up_and_negatives_mirror() {
        // 0.01 ₽ × 0.5 → 0.005 ₽ → rounds up to 0.01 ₽.
        let price = Money::from_minor(1).unwrap();
        assert_eq!(
            line_amount(price, Qty::from_milli(500).unwrap())
                .unwrap()
                .minor(),
            1
        );
        assert_eq!(
            line_amount(price, Qty::from_milli(499).unwrap())
                .unwrap()
                .minor(),
            0
        );
        assert_eq!(
            line_amount(price, Qty::from_milli(-500).unwrap())
                .unwrap()
                .minor(),
            -1
        );
    }

    #[test]
    fn bounds_are_enforced() {
        assert!(Money::from_minor(MAX_MONEY_MINOR).is_ok());
        assert_eq!(
            Money::from_minor(MAX_MONEY_MINOR + 1),
            Err(CrmError::Overflow)
        );
        assert_eq!(
            Money::from_minor(-MAX_MONEY_MINOR - 1),
            Err(CrmError::Overflow)
        );
        assert!(Qty::from_milli(MAX_QTY_MILLI).is_ok());
        assert_eq!(Qty::from_milli(MAX_QTY_MILLI + 1), Err(CrmError::Overflow));
        assert!(Money::non_negative(-1, "price").is_err());
        assert!(Qty::positive(0, "qty").is_err());
        // max price × max qty is far beyond the money bound: must error, not wrap.
        let max_price = Money::from_minor(MAX_MONEY_MINOR).unwrap();
        let max_qty = Qty::from_milli(MAX_QTY_MILLI).unwrap();
        assert_eq!(line_amount(max_price, max_qty), Err(CrmError::Overflow));
    }

    #[test]
    fn checked_arithmetic_rejects_overflow() {
        let max = Money::from_minor(MAX_MONEY_MINOR).unwrap();
        assert_eq!(
            max.checked_add(Money::from_minor(1).unwrap()),
            Err(CrmError::Overflow)
        );
        assert_eq!(
            Money::checked_sum([max, max]),
            Err(CrmError::Overflow),
            "sum beyond the bound must fail"
        );
        assert_eq!(
            Money::checked_sum([
                Money::from_minor(150).unwrap(),
                Money::from_minor(250).unwrap()
            ])
            .unwrap()
            .minor(),
            400
        );
    }

    proptest! {
        #[test]
        fn line_amount_is_exact_for_whole_units(price in 0i64..=10_000_000, units in 0i64..=1_000) {
            let amount = line_amount(
                Money::from_minor(price).unwrap(),
                Qty::from_units(units).unwrap(),
            ).unwrap();
            prop_assert_eq!(amount.minor(), price * units);
        }

        #[test]
        fn line_amount_is_monotonic_in_qty(price in 0i64..=10_000_000, a in 0i64..=1_000_000, b in 0i64..=1_000_000) {
            let price = Money::from_minor(price).unwrap();
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let lo_amount = line_amount(price, Qty::from_milli(lo).unwrap()).unwrap();
            let hi_amount = line_amount(price, Qty::from_milli(hi).unwrap()).unwrap();
            prop_assert!(lo_amount <= hi_amount);
        }

        #[test]
        fn line_amount_error_is_at_most_half_kopeck(price in 0i64..=10_000_000, qty in 0i64..=1_000_000) {
            let amount = line_amount(
                Money::from_minor(price).unwrap(),
                Qty::from_milli(qty).unwrap(),
            ).unwrap();
            let exact_times_1000 = i128::from(price) * i128::from(qty);
            let diff = (i128::from(amount.minor()) * 1000 - exact_times_1000).abs();
            prop_assert!(diff <= 500);
        }
    }
}
