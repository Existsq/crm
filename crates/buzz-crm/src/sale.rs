//! Sale documents (kind 42104) and their reversal (kind 42105).
//!
//! A sale is posted once and never edited: the relay snapshots product names
//! and VAT rates, computes every amount with [`line_amount`] and numbers the
//! sale within the space. A mistake is corrected by voiding the sale
//! (сторно) and posting a new one; the voided sale stays in the history and
//! drops out of analytics.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{CrmError, Result};
use crate::money::{line_amount, Money, Qty};
use crate::space::parse_versioned;

/// Maximum lines on one sale.
pub const MAX_SALE_LINES: usize = 200;
/// Maximum sale note length in characters.
pub const MAX_SALE_NOTE_CHARS: usize = 500;
/// Maximum void reason length in characters.
pub const MAX_VOID_REASON_CHARS: usize = 256;

/// How the customer paid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentMethod {
    /// Cash (наличные).
    Cash,
    /// Bank card (карта).
    Card,
    /// Bank transfer or invoice (безналичный перевод).
    Transfer,
}

impl PaymentMethod {
    /// Every value, in display order.
    pub const ALL: [PaymentMethod; 3] = [
        PaymentMethod::Cash,
        PaymentMethod::Card,
        PaymentMethod::Transfer,
    ];

    /// Canonical wire name (`"cash"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            PaymentMethod::Cash => "cash",
            PaymentMethod::Card => "card",
            PaymentMethod::Transfer => "transfer",
        }
    }
}

/// One line of a sale as submitted by the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaleLine {
    /// Catalog product sold.
    pub product_id: Uuid,
    /// Quantity in thousandths of the product's unit.
    pub qty_milli: Qty,
    /// Price per unit actually charged (may differ from the catalog price).
    pub price_minor: Money,
    /// Discount on the whole line, at most the line's gross amount.
    #[serde(default)]
    pub discount_minor: Money,
}

impl SaleLine {
    /// `price × qty`, rounded half-up to kopecks.
    pub fn gross(&self) -> Result<Money> {
        line_amount(self.price_minor, self.qty_milli)
    }

    /// Gross amount minus the line discount.
    pub fn amount(&self) -> Result<Money> {
        self.gross()?.checked_sub(self.discount_minor)
    }
}

/// Content of a kind 42104 event (the sale id is the `crm-id` tag).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SalePayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Warehouse the goods leave from.
    pub warehouse_id: Uuid,
    /// Buyer; `None` for an anonymous walk-in sale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_id: Option<Uuid>,
    /// Payment method.
    pub payment: PaymentMethod,
    /// Sold goods, 1–[`MAX_SALE_LINES`] lines, one per product.
    pub lines: Vec<SaleLine>,
    /// Optional note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl SalePayload {
    /// Parse and validate kind 42104 content. Product-dependent checks (the
    /// product exists, whole units) run at the relay.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: SalePayload = parse_versioned(content)?;
        if payload.lines.is_empty() || payload.lines.len() > MAX_SALE_LINES {
            return Err(CrmError::field(
                "lines",
                format!("a sale has 1–{MAX_SALE_LINES} lines"),
            ));
        }
        let mut seen = HashSet::new();
        for line in &payload.lines {
            if !seen.insert(line.product_id) {
                return Err(CrmError::field(
                    "lines",
                    format!("product {} appears twice; merge the lines", line.product_id),
                ));
            }
            Qty::positive(line.qty_milli.milli(), "lines.qty_milli")?;
            Money::non_negative(line.price_minor.minor(), "lines.price_minor")?;
            Money::non_negative(line.discount_minor.minor(), "lines.discount_minor")?;
            let gross = line
                .gross()
                .map_err(|_| CrmError::field("lines", "line amount out of range"))?;
            if line.discount_minor > gross {
                return Err(CrmError::field(
                    "lines.discount_minor",
                    "must not exceed the line amount",
                ));
            }
        }
        payload.total()?;
        payload.note = payload
            .note
            .as_deref()
            .map(str::trim)
            .filter(|note| !note.is_empty())
            .map(|note| bounded(note, "note", MAX_SALE_NOTE_CHARS))
            .transpose()?;
        Ok(payload)
    }

    /// Sum of line amounts after discounts.
    pub fn total(&self) -> Result<Money> {
        let amounts = self
            .lines
            .iter()
            .map(SaleLine::amount)
            .collect::<Result<Vec<_>>>()?;
        Money::checked_sum(amounts).map_err(|_| CrmError::field("lines", "sale total out of range"))
    }

    /// Sum of line discounts.
    pub fn discount_total(&self) -> Result<Money> {
        Money::checked_sum(self.lines.iter().map(|line| line.discount_minor))
    }
}

/// Content of a kind 42105 event voiding the sale named by `crm-id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaleVoidPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Why the sale is voided (shown in the sale history).
    pub reason: String,
}

impl SaleVoidPayload {
    /// Parse and validate kind 42105 content.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: SaleVoidPayload = parse_versioned(content)?;
        let reason = payload.reason.trim();
        if reason.is_empty() {
            return Err(CrmError::field("reason", "must not be empty"));
        }
        payload.reason = bounded(reason, "reason", MAX_VOID_REASON_CHARS)?;
        Ok(payload)
    }
}

fn bounded(value: &str, field: &'static str, max: usize) -> Result<String> {
    if value.chars().count() > max || value.chars().any(|c| c.is_control() && c != '\n') {
        return Err(CrmError::field(
            field,
            format!("must be at most {max} printable characters"),
        ));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const P1: &str = "11111111-1111-4111-8111-111111111111";
    const P2: &str = "22222222-2222-4222-8222-222222222222";

    fn base() -> serde_json::Value {
        json!({
            "v": 1,
            "warehouse_id": "33333333-3333-4333-8333-333333333333",
            "customer_id": "44444444-4444-4444-8444-444444444444",
            "payment": "card",
            "lines": [
                {"product_id": P1, "qty_milli": 2000, "price_minor": 8990, "discount_minor": 980},
                {"product_id": P2, "qty_milli": 1250, "price_minor": 59990}
            ],
            "note": "  "
        })
    }

    #[test]
    fn totals_round_each_line_half_up() {
        let sale = SalePayload::parse(&base().to_string()).unwrap();
        // 2 × 89.90 − 9.80 = 170.00; 1.25 × 599.90 = 749.875 → 749.88.
        assert_eq!(sale.lines[0].amount().unwrap().minor(), 17_000);
        assert_eq!(sale.lines[1].amount().unwrap().minor(), 74_988);
        assert_eq!(sale.total().unwrap().minor(), 91_988);
        assert_eq!(sale.discount_total().unwrap().minor(), 980);
        assert_eq!(sale.note, None);
        assert_eq!(sale.payment, PaymentMethod::Card);
    }

    #[test]
    fn anonymous_sale_has_no_customer() {
        let mut value = base();
        value.as_object_mut().unwrap().remove("customer_id");
        assert_eq!(
            SalePayload::parse(&value.to_string()).unwrap().customer_id,
            None
        );
    }

    #[test]
    fn invalid_sales_are_rejected() {
        let line = |qty: i64, price: i64, discount: i64| json!([{"product_id": P1, "qty_milli": qty, "price_minor": price, "discount_minor": discount}]);
        let cases: [(&str, serde_json::Value); 8] = [
            ("lines", json!([])),
            ("lines", line(0, 100, 0)),
            ("lines", line(-1000, 100, 0)),
            ("lines", line(1000, -1, 0)),
            ("lines", line(1000, 100, 101)),
            (
                "lines",
                json!([
                    {"product_id": P1, "qty_milli": 1000, "price_minor": 1},
                    {"product_id": P1, "qty_milli": 1000, "price_minor": 1}
                ]),
            ),
            ("payment", json!("barter")),
            ("note", json!("x".repeat(MAX_SALE_NOTE_CHARS + 1))),
        ];
        for (field, value) in cases {
            let mut payload = base();
            payload[field] = value.clone();
            assert!(
                SalePayload::parse(&payload.to_string()).is_err(),
                "{field} = {value} should be rejected"
            );
        }
    }

    #[test]
    fn too_many_lines_are_rejected() {
        let lines: Vec<_> = (0..=MAX_SALE_LINES)
            .map(|_| json!({"product_id": Uuid::new_v4(), "qty_milli": 1000, "price_minor": 1}))
            .collect();
        let mut value = base();
        value["lines"] = json!(lines);
        assert!(SalePayload::parse(&value.to_string()).is_err());
    }

    #[test]
    fn void_requires_a_reason() {
        assert_eq!(
            SaleVoidPayload::parse(r#"{"v":1,"reason":"  ошибка кассира "}"#)
                .unwrap()
                .reason,
            "ошибка кассира"
        );
        assert!(SaleVoidPayload::parse(r#"{"v":1,"reason":"  "}"#).is_err());
        assert!(SaleVoidPayload::parse(r#"{"v":1}"#).is_err());
    }

    #[test]
    fn payment_names_round_trip() {
        for method in PaymentMethod::ALL {
            let json = serde_json::to_string(&method).unwrap();
            assert_eq!(json, format!("\"{}\"", method.as_str()));
        }
    }
}
