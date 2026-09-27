//! Product catalog payload (kind 42102).

use serde::{Deserialize, Serialize};

use crate::error::{CrmError, Result};
use crate::money::{Money, Qty};
use crate::space::parse_versioned;
use crate::validate;
use crate::vat::VatRate;

/// Maximum barcodes per product.
pub const MAX_BARCODES: usize = 20;
/// Maximum SKU length in characters.
pub const MAX_SKU_CHARS: usize = 64;
/// Maximum category length in characters.
pub const MAX_CATEGORY_CHARS: usize = 64;

/// Unit of measure a product is sold in.
///
/// Mapped onto FFD 1.2 measure codes by the ATOL integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// Piece (шт) — sold in whole units only.
    Piece,
    /// Kilogram.
    Kilogram,
    /// Gram.
    Gram,
    /// Liter.
    Liter,
    /// Milliliter.
    Milliliter,
    /// Meter.
    Meter,
    /// Square meter.
    SquareMeter,
    /// Hour of work.
    Hour,
    /// One unit of a service.
    ServiceUnit,
}

impl Unit {
    /// Every unit, in display order.
    pub const ALL: [Unit; 9] = [
        Unit::Piece,
        Unit::Kilogram,
        Unit::Gram,
        Unit::Liter,
        Unit::Milliliter,
        Unit::Meter,
        Unit::SquareMeter,
        Unit::Hour,
        Unit::ServiceUnit,
    ];

    /// Canonical wire name (`"piece"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Unit::Piece => "piece",
            Unit::Kilogram => "kilogram",
            Unit::Gram => "gram",
            Unit::Liter => "liter",
            Unit::Milliliter => "milliliter",
            Unit::Meter => "meter",
            Unit::SquareMeter => "square_meter",
            Unit::Hour => "hour",
            Unit::ServiceUnit => "service_unit",
        }
    }

    /// Whether quantities in this unit must be whole numbers.
    pub const fn whole_only(self) -> bool {
        matches!(self, Unit::Piece | Unit::ServiceUnit)
    }

    /// Validate a quantity in this unit (whole units where required).
    pub fn check_qty(self, qty: Qty, field: &'static str) -> Result<()> {
        if self.whole_only() && !qty.is_whole() {
            return Err(CrmError::field(
                field,
                format!("{} is sold in whole units", self.as_str()),
            ));
        }
        Ok(())
    }
}

/// Fiscal item type (признак предмета расчёта, FFD tag 1212).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentObject {
    /// Goods (товар).
    Commodity,
    /// Excisable goods (подакцизный товар).
    Excise,
    /// Work (работа).
    Job,
    /// Service (услуга).
    Service,
    /// Anything else (иной предмет расчёта).
    Another,
}

impl PaymentObject {
    /// Every value, in display order.
    pub const ALL: [PaymentObject; 5] = [
        PaymentObject::Commodity,
        PaymentObject::Excise,
        PaymentObject::Job,
        PaymentObject::Service,
        PaymentObject::Another,
    ];

    /// Canonical wire name (`"commodity"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            PaymentObject::Commodity => "commodity",
            PaymentObject::Excise => "excise",
            PaymentObject::Job => "job",
            PaymentObject::Service => "service",
            PaymentObject::Another => "another",
        }
    }
}

/// Content of a kind 42102 event (the product id is the `crm-id` tag).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Display and receipt name (1–128 characters).
    pub name: String,
    /// Optional article number, unique per space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sku: Option<String>,
    /// Barcodes, unique per space.
    #[serde(default)]
    pub barcodes: Vec<String>,
    /// Unit of measure.
    pub unit: Unit,
    /// Sale price per unit.
    pub price_minor: Money,
    /// VAT rate for sales of this product.
    pub vat: VatRate,
    /// Fiscal item type.
    pub payment_object: PaymentObject,
    /// Optional free-text category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Stock level at or below which the product is "running low".
    #[serde(default)]
    pub min_stock_milli: Qty,
    /// Goods that need a marking code (Честный знак); v1 POS refuses them.
    #[serde(default)]
    pub requires_marking: bool,
    /// Archived products are hidden from lists and cannot be sold.
    #[serde(default)]
    pub archived: bool,
}

impl ProductPayload {
    /// Parse, normalize and validate kind 42102 content.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: ProductPayload = parse_versioned(content)?;
        payload.name = validate::name(&payload.name, "name")?;
        payload.sku = payload.sku.as_deref().map(sku).transpose()?.flatten();
        payload.category = payload
            .category
            .as_deref()
            .map(category)
            .transpose()?
            .flatten();
        payload.barcodes = barcodes(&payload.barcodes)?;
        Money::non_negative(payload.price_minor.minor(), "price_minor")?;
        if payload.min_stock_milli.milli() < 0 {
            return Err(CrmError::field("min_stock_milli", "must not be negative"));
        }
        Qty::from_milli(payload.min_stock_milli.milli())
            .map_err(|_| CrmError::field("min_stock_milli", "out of range"))?;
        payload
            .unit
            .check_qty(payload.min_stock_milli, "min_stock_milli")?;
        if payload.vat.is_historical() {
            return Err(CrmError::field(
                "vat",
                format!("{} is historical; use the current rate", payload.vat),
            ));
        }
        Ok(payload)
    }
}

/// Trimmed SKU, or `None` when blank.
fn sku(raw: &str) -> Result<Option<String>> {
    let value = raw.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let valid = value.chars().count() <= MAX_SKU_CHARS
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'));
    if !valid {
        return Err(CrmError::field(
            "sku",
            format!("must be at most {MAX_SKU_CHARS} characters of A-Z, 0-9, . _ / -"),
        ));
    }
    Ok(Some(value.to_owned()))
}

fn category(raw: &str) -> Result<Option<String>> {
    let value = raw.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > MAX_CATEGORY_CHARS || value.chars().any(char::is_control) {
        return Err(CrmError::field(
            "category",
            format!("must be at most {MAX_CATEGORY_CHARS} printable characters"),
        ));
    }
    Ok(Some(value.to_owned()))
}

/// Validate, trim and de-duplicate barcodes (order preserved).
fn barcodes(raw: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for code in raw {
        let code = code.trim();
        barcode(code)?;
        if !out.iter().any(|seen| seen == code) {
            out.push(code.to_owned());
        }
    }
    if out.len() > MAX_BARCODES {
        return Err(CrmError::field(
            "barcodes",
            format!("at most {MAX_BARCODES} barcodes per product"),
        ));
    }
    Ok(out)
}

/// Validate one barcode: GS1 codes (8/12/13/14 digits) must carry a correct
/// check digit; other codes are 4–32 characters of `A-Z a-z 0-9 -`.
pub fn barcode(code: &str) -> Result<()> {
    let numeric = !code.is_empty() && code.chars().all(|c| c.is_ascii_digit());
    if numeric && matches!(code.len(), 8 | 12 | 13 | 14) {
        if gs1_check_digit_ok(code) {
            return Ok(());
        }
        return Err(CrmError::field(
            "barcodes",
            format!("{code} has a wrong check digit"),
        ));
    }
    let valid = (4..=32).contains(&code.len())
        && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if valid {
        Ok(())
    } else {
        Err(CrmError::field(
            "barcodes",
            format!("{code:?} is not a valid barcode"),
        ))
    }
}

fn gs1_check_digit_ok(code: &str) -> bool {
    let digits: Vec<u32> = code.chars().filter_map(|c| c.to_digit(10)).collect();
    let Some((&check, body)) = digits.split_last() else {
        return false;
    };
    let sum: u32 = body
        .iter()
        .rev()
        .enumerate()
        .map(|(index, digit)| if index % 2 == 0 { digit * 3 } else { *digit })
        .sum();
    (10 - sum % 10) % 10 == check
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base() -> serde_json::Value {
        json!({
            "v": 1,
            "name": "  Молоко 3,2% 1 л ",
            "sku": " MLK-001 ",
            "barcodes": ["4600051000057", " 4600051000057", "INT-42"],
            "unit": "piece",
            "price_minor": 8990,
            "vat": "vat10",
            "payment_object": "commodity",
            "category": " Молочные продукты ",
            "min_stock_milli": 5000
        })
    }

    #[test]
    fn valid_product_is_normalized() {
        let product = ProductPayload::parse(&base().to_string()).unwrap();
        assert_eq!(product.name, "Молоко 3,2% 1 л");
        assert_eq!(product.sku.as_deref(), Some("MLK-001"));
        assert_eq!(product.barcodes, vec!["4600051000057", "INT-42"]);
        assert_eq!(product.category.as_deref(), Some("Молочные продукты"));
        assert_eq!(product.price_minor.minor(), 8990);
        assert!(!product.requires_marking && !product.archived);
    }

    #[test]
    fn blank_optional_fields_become_none() {
        let mut value = base();
        value["sku"] = json!("   ");
        value["category"] = json!("");
        let product = ProductPayload::parse(&value.to_string()).unwrap();
        assert_eq!(product.sku, None);
        assert_eq!(product.category, None);
    }

    #[test]
    fn invalid_fields_are_rejected() {
        let cases: [(&str, serde_json::Value); 11] = [
            ("name", json!("")),
            ("sku", json!("bad sku!")),
            ("barcodes", json!(["4600051000058"])),
            ("barcodes", json!(["ab"])),
            ("price_minor", json!(-1)),
            ("vat", json!("vat20")),
            ("unit", json!("bucket")),
            ("payment_object", json!("gift")),
            ("min_stock_milli", json!(1500)),
            ("min_stock_milli", json!(1_000_001_000)),
            ("price_minor", json!(100_000_000_001_i64)),
        ];
        for (field, value) in cases {
            let mut payload = base();
            payload[field] = value.clone();
            assert!(
                ProductPayload::parse(&payload.to_string()).is_err(),
                "{field} = {value} should be rejected"
            );
        }
    }

    #[test]
    fn fractional_quantities_are_allowed_for_weighed_goods() {
        let mut value = base();
        value["unit"] = json!("kilogram");
        value["min_stock_milli"] = json!(1500);
        assert!(ProductPayload::parse(&value.to_string()).is_ok());
    }

    #[test]
    fn too_many_barcodes_are_rejected() {
        let mut value = base();
        let codes: Vec<String> = (0..=MAX_BARCODES).map(|i| format!("CODE-{i:03}")).collect();
        value["barcodes"] = json!(codes);
        assert!(ProductPayload::parse(&value.to_string()).is_err());
    }

    #[test]
    fn gs1_check_digits() {
        for valid in [
            "4006381333931",
            "036000291452",
            "96385074",
            "10012345000017",
            "2000000000015",
        ] {
            assert!(barcode(valid).is_ok(), "{valid}");
        }
        for invalid in ["4006381333932", "4607001234565", "96385075"] {
            assert!(barcode(invalid).is_err(), "{invalid}");
        }
        // Non-GS1 lengths fall back to the internal-code rule.
        assert!(barcode("1234567").is_ok());
        assert!(barcode("ABC").is_err());
    }

    #[test]
    fn unit_and_payment_object_names_round_trip() {
        for unit in Unit::ALL {
            let json = serde_json::to_string(&unit).unwrap();
            assert_eq!(json, format!("\"{}\"", unit.as_str()));
        }
        for object in PaymentObject::ALL {
            let json = serde_json::to_string(&object).unwrap();
            assert_eq!(json, format!("\"{}\"", object.as_str()));
        }
    }
}
