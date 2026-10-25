//! Stock ledger: stock documents (kinds 42106, 42107), service tech cards
//! (kind 42108) and the moving-average arithmetic the relay posts them with.
//!
//! A stock level is a quantity and the total cost value of that quantity on
//! one warehouse. Receipts add value at their purchase cost; every outflow
//! (sale, write-off, transfer, consumables of a paid service) takes value at
//! the current average cost. A level whose quantity is zero or negative holds
//! no value. Documents are posted once and corrected by voiding them, which
//! reverts exactly the movements they made.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::catalog::PaymentObject;
use crate::error::{CrmError, Result};
use crate::money::{line_amount, Money, Qty, QTY_SCALE};
use crate::sale::bounded;
use crate::space::parse_versioned;

/// Maximum lines on one stock document.
pub const MAX_STOCK_DOC_LINES: usize = 500;
/// Maximum note length in characters.
pub const MAX_STOCK_NOTE_CHARS: usize = 500;
/// Maximum supplier document reference (invoice number) length.
pub const MAX_REFERENCE_CHARS: usize = 64;
/// Maximum consumables on one tech card.
pub const MAX_TECH_CARD_ITEMS: usize = 50;

/// What a stock document does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockDocType {
    /// Goods received from a supplier (поступление), at purchase cost.
    Receipt,
    /// Goods written off: damage, expiry, own use (списание).
    WriteOff,
    /// Counted quantities replace the book quantities (инвентаризация).
    Inventory,
    /// Goods moved to another warehouse (перемещение).
    Transfer,
}

impl StockDocType {
    /// Every value, in display order.
    pub const ALL: [StockDocType; 4] = [
        StockDocType::Receipt,
        StockDocType::WriteOff,
        StockDocType::Inventory,
        StockDocType::Transfer,
    ];

    /// Canonical wire name (`"write_off"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            StockDocType::Receipt => "receipt",
            StockDocType::WriteOff => "write_off",
            StockDocType::Inventory => "inventory",
            StockDocType::Transfer => "transfer",
        }
    }
}

/// Whether products of this fiscal type are kept in stock. Services and
/// work are not; "another" covers non-goods such as deposits.
pub const fn is_stocked(payment_object: PaymentObject) -> bool {
    matches!(
        payment_object,
        PaymentObject::Commodity | PaymentObject::Excise
    )
}

/// One line of a stock document as submitted by the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockDocLine {
    /// Stocked catalog product.
    pub product_id: Uuid,
    /// Quantity moved, in thousandths; for an inventory, the counted
    /// quantity (zero allowed).
    pub qty_milli: Qty,
    /// Purchase cost per unit: required on a receipt; on an inventory, the
    /// cost of a surplus (the average cost when absent); not allowed elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_minor: Option<Money>,
}

/// Content of a kind 42106 event (the document id is the `crm-id` tag).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockDocPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// What the document does.
    #[serde(rename = "type")]
    pub doc_type: StockDocType,
    /// Warehouse the stock changes on (the source of a transfer).
    pub warehouse_id: Uuid,
    /// Destination of a transfer; only on transfers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_warehouse_id: Option<Uuid>,
    /// Supplier of a receipt; only on receipts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supplier_id: Option<Uuid>,
    /// Supplier's invoice number or other reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// 1–[`MAX_STOCK_DOC_LINES`] lines, one per product.
    pub lines: Vec<StockDocLine>,
    /// Optional note (the reason of a write-off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl StockDocPayload {
    /// Parse and validate kind 42106 content. Product-dependent checks (the
    /// product exists and is stocked, whole units) run at the relay.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: StockDocPayload = parse_versioned(content)?;
        if payload.lines.is_empty() || payload.lines.len() > MAX_STOCK_DOC_LINES {
            return Err(CrmError::field(
                "lines",
                format!("a stock document has 1–{MAX_STOCK_DOC_LINES} lines"),
            ));
        }
        let doc_type = payload.doc_type;
        match (doc_type, payload.to_warehouse_id) {
            (StockDocType::Transfer, None) => {
                return Err(CrmError::field("to_warehouse_id", "required on a transfer"))
            }
            (StockDocType::Transfer, Some(to)) if to == payload.warehouse_id => {
                return Err(CrmError::field(
                    "to_warehouse_id",
                    "must differ from warehouse_id",
                ))
            }
            (StockDocType::Transfer, Some(_)) | (_, None) => {}
            (_, Some(_)) => return Err(CrmError::field("to_warehouse_id", "only on a transfer")),
        }
        if payload.supplier_id.is_some() && doc_type != StockDocType::Receipt {
            return Err(CrmError::field("supplier_id", "only on a receipt"));
        }
        let mut seen = HashSet::new();
        for line in &payload.lines {
            if !seen.insert(line.product_id) {
                return Err(CrmError::field(
                    "lines",
                    format!("product {} appears twice; merge the lines", line.product_id),
                ));
            }
            if doc_type == StockDocType::Inventory {
                if line.qty_milli.milli() < 0 {
                    return Err(CrmError::field("lines.qty_milli", "must not be negative"));
                }
                Qty::from_milli(line.qty_milli.milli())
                    .map_err(|_| CrmError::field("lines.qty_milli", "out of range"))?;
            } else {
                Qty::positive(line.qty_milli.milli(), "lines.qty_milli")?;
            }
            match (doc_type, line.cost_minor) {
                (StockDocType::Receipt, None) => {
                    return Err(CrmError::field("lines.cost_minor", "required on a receipt"))
                }
                (StockDocType::Receipt | StockDocType::Inventory, Some(cost)) => {
                    Money::non_negative(cost.minor(), "lines.cost_minor")?;
                    line_amount(cost, line.qty_milli)
                        .map_err(|_| CrmError::field("lines", "line amount out of range"))?;
                }
                (StockDocType::WriteOff | StockDocType::Transfer, Some(_)) => {
                    return Err(CrmError::field(
                        "lines.cost_minor",
                        "only on a receipt or an inventory",
                    ))
                }
                (_, None) => {}
            }
        }
        payload.reference = optional_text(
            payload.reference.as_deref(),
            "reference",
            MAX_REFERENCE_CHARS,
        )?;
        if payload
            .reference
            .as_deref()
            .is_some_and(|r| r.contains('\n'))
        {
            return Err(CrmError::field("reference", "must be one line"));
        }
        payload.note = optional_text(payload.note.as_deref(), "note", MAX_STOCK_NOTE_CHARS)?;
        Ok(payload)
    }
}

fn optional_text(raw: Option<&str>, field: &'static str, max: usize) -> Result<Option<String>> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| bounded(value, field, max))
        .transpose()
}

/// One consumable of a tech card: how much of a stocked product one unit of
/// the service uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechCardItem {
    /// Stocked catalog product.
    pub product_id: Uuid,
    /// Quantity per service unit, in thousandths.
    pub qty_milli: Qty,
}

/// Content of a kind 42108 event (the service product id is the `crm-id`
/// tag). An empty list removes the consumables of the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechCardPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Consumables, 0–[`MAX_TECH_CARD_ITEMS`], one per product.
    #[serde(default)]
    pub items: Vec<TechCardItem>,
}

impl TechCardPayload {
    /// Parse and validate kind 42108 content.
    pub fn parse(content: &str) -> Result<Self> {
        let payload: TechCardPayload = parse_versioned(content)?;
        if payload.items.len() > MAX_TECH_CARD_ITEMS {
            return Err(CrmError::field(
                "items",
                format!("a tech card has at most {MAX_TECH_CARD_ITEMS} consumables"),
            ));
        }
        let mut seen = HashSet::new();
        for item in &payload.items {
            if !seen.insert(item.product_id) {
                return Err(CrmError::field(
                    "items",
                    format!("product {} appears twice", item.product_id),
                ));
            }
            Qty::positive(item.qty_milli.milli(), "items.qty_milli")?;
        }
        Ok(payload)
    }
}

/// `per_unit × units`, both in thousandths, rounded half-up to a thousandth:
/// the consumables of `units` of a service.
pub fn scale_qty(per_unit: Qty, units: Qty) -> Result<Qty> {
    let raw = i128::from(per_unit.milli()) * i128::from(units.milli());
    let scale = i128::from(QTY_SCALE);
    let rounded = (raw + scale / 2) / scale;
    Qty::from_milli(i64::try_from(rounded).map_err(|_| CrmError::Overflow)?)
}

/// A signed change of one stock level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StockDelta {
    /// Quantity change in thousandths.
    pub qty: Qty,
    /// Value change in kopecks.
    pub value: Money,
}

impl StockDelta {
    /// True when nothing changes.
    pub fn is_zero(self) -> bool {
        self.qty == Qty::ZERO && self.value == Money::ZERO
    }
}

/// The quantity and cost value of one product on one warehouse.
///
/// Invariant: `value >= 0`, and `value == 0` whenever `qty <= 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StockLevel {
    /// Quantity in thousandths (negative when oversold).
    pub qty: Qty,
    /// Cost value of the quantity, kopecks.
    pub value: Money,
}

impl StockLevel {
    /// Build a level from stored columns.
    pub fn new(qty_milli: i64, value_minor: i64) -> Result<Self> {
        Ok(Self {
            qty: Qty::from_milli(qty_milli)?,
            value: Money::from_minor(value_minor)?,
        })
    }

    /// Average cost of one unit, rounded half-up; `None` without stock.
    pub fn average_cost(self) -> Option<Money> {
        if self.qty.milli() <= 0 {
            return None;
        }
        let raw = i128::from(self.value.minor()) * i128::from(QTY_SCALE);
        let qty = i128::from(self.qty.milli());
        let minor = i64::try_from((raw + qty / 2) / qty).ok()?;
        Money::from_minor(minor).ok()
    }

    /// The delta that brings this level to `target`, keeping the invariant.
    fn delta_to(self, qty: i64, value: i64) -> Result<StockDelta> {
        let qty = Qty::from_milli(qty)?;
        let value = if qty.milli() <= 0 { 0 } else { value.max(0) };
        Ok(StockDelta {
            qty: Qty::from_milli(qty.milli() - self.qty.milli())?,
            value: Money::from_minor(value - self.value.minor())?,
        })
    }

    /// Receive `qty` worth `value` in total.
    pub fn receive(self, qty: Qty, value: Money) -> Result<StockDelta> {
        let after = self
            .qty
            .milli()
            .checked_add(qty.milli())
            .ok_or(CrmError::Overflow)?;
        let new_value = if self.qty.milli() >= 0 {
            self.value.minor() + value.minor()
        } else if after > 0 {
            // Oversold stock: only the part that ends up on hand keeps value.
            let raw = i128::from(value.minor()) * i128::from(after);
            i64::try_from(raw / i128::from(qty.milli().max(1))).map_err(|_| CrmError::Overflow)?
        } else {
            0
        };
        self.delta_to(after, new_value)
    }

    /// Issue `qty` at the average cost. `allow_negative` decides whether the
    /// level may go below zero.
    pub fn issue(self, qty: Qty, allow_negative: bool) -> Result<StockDelta> {
        let after = self
            .qty
            .milli()
            .checked_sub(qty.milli())
            .ok_or(CrmError::Overflow)?;
        if after < 0 && !allow_negative {
            return Err(CrmError::field(
                "qty_milli",
                format!(
                    "not enough stock: {} on hand, {} requested",
                    self.qty.milli(),
                    qty.milli()
                ),
            ));
        }
        let cost = if after <= 0 {
            self.value.minor()
        } else {
            let raw = i128::from(self.value.minor()) * i128::from(qty.milli());
            let on_hand = i128::from(self.qty.milli());
            i64::try_from((raw + on_hand / 2) / on_hand).map_err(|_| CrmError::Overflow)?
        };
        self.delta_to(after, self.value.minor() - cost)
    }

    /// Set the quantity to a counted one. A surplus is valued at `unit_cost`
    /// or, when absent, at the average cost (zero without stock).
    pub fn count(self, counted: Qty, unit_cost: Option<Money>) -> Result<StockDelta> {
        let diff = counted.milli() - self.qty.milli();
        match diff.cmp(&0) {
            std::cmp::Ordering::Equal => Ok(StockDelta::default()),
            std::cmp::Ordering::Less => self.issue(Qty::from_milli(-diff)?, true),
            std::cmp::Ordering::Greater => {
                let diff = Qty::from_milli(diff)?;
                let cost = unit_cost.or(self.average_cost()).unwrap_or(Money::ZERO);
                self.receive(diff, line_amount(cost, diff)?)
            }
        }
    }

    /// The delta that undoes `delta` as far as the invariant allows (value
    /// already issued at a different cost is not recreated).
    pub fn revert(self, delta: StockDelta, allow_negative: bool) -> Result<StockDelta> {
        let after = self
            .qty
            .milli()
            .checked_sub(delta.qty.milli())
            .ok_or(CrmError::Overflow)?;
        if after < 0 && after < self.qty.milli() && !allow_negative {
            return Err(CrmError::field(
                "qty_milli",
                format!(
                    "not enough stock to revert: {} on hand, {} to take back",
                    self.qty.milli(),
                    delta.qty.milli()
                ),
            ));
        }
        self.delta_to(after, self.value.minor() - delta.value.minor())
    }

    /// The level after `delta`.
    pub fn apply(self, delta: StockDelta) -> Result<StockLevel> {
        Ok(StockLevel {
            qty: Qty::from_milli(
                self.qty
                    .milli()
                    .checked_add(delta.qty.milli())
                    .ok_or(CrmError::Overflow)?,
            )?,
            value: self.value.checked_add(delta.value)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const P1: &str = "11111111-1111-4111-8111-111111111111";
    const P2: &str = "22222222-2222-4222-8222-222222222222";
    const W1: &str = "33333333-3333-4333-8333-333333333333";
    const W2: &str = "44444444-4444-4444-8444-444444444444";

    fn level(qty: i64, value: i64) -> StockLevel {
        StockLevel::new(qty, value).unwrap()
    }

    fn qty(milli: i64) -> Qty {
        Qty::from_milli(milli).unwrap()
    }

    fn money(minor: i64) -> Money {
        Money::from_minor(minor).unwrap()
    }

    fn after(start: StockLevel, delta: StockDelta) -> (i64, i64) {
        let end = start.apply(delta).unwrap();
        (end.qty.milli(), end.value.minor())
    }

    fn receipt() -> serde_json::Value {
        json!({
            "v": 1,
            "type": "receipt",
            "warehouse_id": W1,
            "supplier_id": W2,
            "reference": " ТН-15 ",
            "lines": [
                {"product_id": P1, "qty_milli": 10_000, "cost_minor": 25_000},
                {"product_id": P2, "qty_milli": 1_500, "cost_minor": 0}
            ]
        })
    }

    #[test]
    fn receipts_need_costs_and_normalize_text() {
        let doc = StockDocPayload::parse(&receipt().to_string()).unwrap();
        assert_eq!(doc.doc_type, StockDocType::Receipt);
        assert_eq!(doc.reference.as_deref(), Some("ТН-15"));
        assert_eq!(doc.note, None);

        let mut value = receipt();
        value["lines"][0]
            .as_object_mut()
            .unwrap()
            .remove("cost_minor");
        assert!(StockDocPayload::parse(&value.to_string()).is_err());
    }

    #[test]
    fn document_shapes_are_checked_per_type() {
        let bad = |patch: serde_json::Value| {
            let mut value = receipt();
            for (key, field) in patch.as_object().unwrap() {
                value[key] = field.clone();
            }
            StockDocPayload::parse(&value.to_string()).is_err()
        };
        assert!(bad(json!({"lines": []})));
        assert!(bad(json!({"to_warehouse_id": W2})));
        assert!(bad(json!({"type": "write_off"})), "supplier on a write-off");
        assert!(bad(json!({"type": "transfer", "supplier_id": null})));
        assert!(bad(
            json!({"type": "transfer", "supplier_id": null, "to_warehouse_id": W1})
        ));
        assert!(bad(json!({"reference": "a\nb"})));
        assert!(bad(json!({"lines": [
            {"product_id": P1, "qty_milli": 1000, "cost_minor": 1},
            {"product_id": P1, "qty_milli": 1000, "cost_minor": 1}
        ]})));
        assert!(bad(
            json!({"lines": [{"product_id": P1, "qty_milli": 0, "cost_minor": 1}]})
        ));

        let write_off = json!({
            "v": 1, "type": "write_off", "warehouse_id": W1, "note": "бой",
            "lines": [{"product_id": P1, "qty_milli": 1000}]
        });
        assert!(StockDocPayload::parse(&write_off.to_string()).is_ok());
        let mut costed = write_off.clone();
        costed["lines"][0]["cost_minor"] = json!(100);
        assert!(StockDocPayload::parse(&costed.to_string()).is_err());

        let inventory = json!({
            "v": 1, "type": "inventory", "warehouse_id": W1,
            "lines": [{"product_id": P1, "qty_milli": 0}, {"product_id": P2, "qty_milli": 3000, "cost_minor": 500}]
        });
        assert!(StockDocPayload::parse(&inventory.to_string()).is_ok());

        let transfer = json!({
            "v": 1, "type": "transfer", "warehouse_id": W1, "to_warehouse_id": W2,
            "lines": [{"product_id": P1, "qty_milli": 1000}]
        });
        assert!(StockDocPayload::parse(&transfer.to_string()).is_ok());
    }

    #[test]
    fn tech_cards_list_each_consumable_once() {
        let card = |items: serde_json::Value| {
            TechCardPayload::parse(&json!({"v": 1, "items": items}).to_string())
        };
        assert_eq!(card(json!([])).unwrap().items.len(), 0);
        assert!(card(json!([{"product_id": P1, "qty_milli": 30}])).is_ok());
        assert!(card(json!([{"product_id": P1, "qty_milli": 0}])).is_err());
        assert!(card(json!([
            {"product_id": P1, "qty_milli": 1},
            {"product_id": P1, "qty_milli": 2}
        ]))
        .is_err());
        assert!(TechCardPayload::parse(r#"{"v":1}"#).is_ok());
    }

    #[test]
    fn consumables_scale_with_the_service_quantity() {
        assert_eq!(scale_qty(qty(30), qty(2_000)).unwrap(), qty(60));
        assert_eq!(
            scale_qty(qty(1), qty(500)).unwrap(),
            qty(1),
            "0.5 rounds up"
        );
        assert_eq!(scale_qty(qty(1), qty(400)).unwrap(), qty(0));
    }

    #[test]
    fn receipts_build_a_moving_average() {
        let start = StockLevel::default();
        let first = start.receive(qty(10_000), money(25_000)).unwrap();
        let level = start.apply(first).unwrap();
        assert_eq!(level.average_cost(), Some(money(2_500)));
        let second = level.receive(qty(10_000), money(35_000)).unwrap();
        let level = level.apply(second).unwrap();
        assert_eq!((level.qty.milli(), level.value.minor()), (20_000, 60_000));
        assert_eq!(level.average_cost(), Some(money(3_000)));
    }

    #[test]
    fn issues_take_the_average_cost_and_empty_stock_holds_no_value() {
        let start = level(3_000, 1_000);
        let issue = start.issue(qty(1_000), false).unwrap();
        assert_eq!(issue.value, money(-333));
        assert_eq!(after(start, issue), (2_000, 667));
        let all = start.issue(qty(3_000), false).unwrap();
        assert_eq!(after(start, all), (0, 0));
        assert!(start.issue(qty(3_001), false).is_err());
        let oversold = start.issue(qty(5_000), true).unwrap();
        assert_eq!(after(start, oversold), (-2_000, 0));
    }

    #[test]
    fn receiving_into_oversold_stock_values_only_what_is_on_hand() {
        let start = level(-2_000, 0);
        let delta = start.receive(qty(5_000), money(500)).unwrap();
        assert_eq!(after(start, delta), (3_000, 300));
        let still_short = start.receive(qty(1_000), money(100)).unwrap();
        assert_eq!(after(start, still_short), (-1_000, 0));
    }

    #[test]
    fn counts_book_surpluses_and_shortages() {
        let start = level(4_000, 400);
        assert!(start.count(qty(4_000), None).unwrap().is_zero());
        assert_eq!(
            after(start, start.count(qty(1_000), None).unwrap()),
            (1_000, 100)
        );
        assert_eq!(
            after(start, start.count(qty(6_000), None).unwrap()),
            (6_000, 600)
        );
        assert_eq!(
            after(start, start.count(qty(6_000), Some(money(50))).unwrap()),
            (6_000, 500)
        );
        let empty = level(-1_000, 0);
        assert_eq!(
            after(empty, empty.count(qty(2_000), None).unwrap()),
            (2_000, 0)
        );
    }

    #[test]
    fn reverting_restores_the_level_and_respects_the_stock_rule() {
        let start = level(2_000, 600);
        let issue = start.issue(qty(1_000), false).unwrap();
        let issued = start.apply(issue).unwrap();
        let back = issued.revert(issue, false).unwrap();
        assert_eq!(after(issued, back), (2_000, 600));

        let receipt = StockLevel::default()
            .receive(qty(5_000), money(500))
            .unwrap();
        let sold_out = level(1_000, 100);
        assert!(sold_out.revert(receipt, false).is_err());
        assert_eq!(
            after(sold_out, sold_out.revert(receipt, true).unwrap()),
            (-4_000, 0)
        );
    }

    #[test]
    fn type_names_round_trip() {
        for doc_type in StockDocType::ALL {
            let json = serde_json::to_string(&doc_type).unwrap();
            assert_eq!(json, format!("\"{}\"", doc_type.as_str()));
        }
        assert!(is_stocked(PaymentObject::Commodity));
        assert!(!is_stocked(PaymentObject::Service));
    }
}
