//! Customer and supplier payload (kind 42103).

use serde::{Deserialize, Serialize};

use crate::error::{CrmError, Result};
use crate::space::parse_versioned;
use crate::validate;

/// Maximum phone length in characters (as typed, before digit extraction).
pub const MAX_PHONE_CHARS: usize = 32;
/// Maximum email length in characters.
pub const MAX_EMAIL_CHARS: usize = 254;
/// Maximum address length in characters.
pub const MAX_ADDRESS_CHARS: usize = 256;
/// Maximum note length in characters.
pub const MAX_NOTE_CHARS: usize = 2000;

/// What a counterparty is to the shop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartyType {
    /// Buys from the shop (покупатель).
    Customer,
    /// Sells to the shop (поставщик).
    Supplier,
    /// Both a customer and a supplier.
    Both,
}

impl PartyType {
    /// Every value, in display order.
    pub const ALL: [PartyType; 3] = [PartyType::Customer, PartyType::Supplier, PartyType::Both];

    /// Canonical wire name (`"customer"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            PartyType::Customer => "customer",
            PartyType::Supplier => "supplier",
            PartyType::Both => "both",
        }
    }

    /// Whether the party may be named as the buyer on a sale.
    pub const fn buys(self) -> bool {
        matches!(self, PartyType::Customer | PartyType::Both)
    }
}

/// Content of a kind 42103 event (the party id is the `crm-id` tag).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartyPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Customer, supplier or both.
    #[serde(rename = "type")]
    pub party_type: PartyType,
    /// Person's full name or company name (1–128 characters).
    pub name: String,
    /// Phone as typed; at least 5 digits when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// Email address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Taxpayer number (ИНН), unique per space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inn: Option<String>,
    /// Postal or delivery address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// Free-text note (preferences, contract terms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Archived parties are hidden from lists and cannot be put on new sales.
    #[serde(default)]
    pub archived: bool,
}

impl PartyPayload {
    /// Parse, normalize and validate kind 42103 content.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: PartyPayload = parse_versioned(content)?;
        payload.name = validate::name(&payload.name, "name")?;
        payload.phone = optional(payload.phone.as_deref()).map(phone).transpose()?;
        payload.email = optional(payload.email.as_deref()).map(email).transpose()?;
        payload.inn = optional(payload.inn.as_deref())
            .map(|inn| validate::inn(inn).map(|()| inn.to_owned()))
            .transpose()?;
        payload.address = optional(payload.address.as_deref())
            .map(|value| text(value, "address", MAX_ADDRESS_CHARS, false))
            .transpose()?;
        payload.note = optional(payload.note.as_deref())
            .map(|value| text(value, "note", MAX_NOTE_CHARS, true))
            .transpose()?;
        Ok(payload)
    }
}

/// The digits of a phone number, used for search (`+7 (912) 000-11-22` →
/// `79120001122`).
pub fn phone_digits(phone: &str) -> String {
    phone.chars().filter(char::is_ascii_digit).collect()
}

fn optional(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|value| !value.is_empty())
}

fn phone(value: &str) -> Result<String> {
    let allowed = value
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | '(' | ')' | '-' | ' '));
    let digits = phone_digits(value).len();
    if !allowed || value.chars().count() > MAX_PHONE_CHARS || !(5..=15).contains(&digits) {
        return Err(CrmError::field(
            "phone",
            "must be 5–15 digits, optionally with + ( ) - and spaces",
        ));
    }
    Ok(value.to_owned())
}

fn email(value: &str) -> Result<String> {
    let valid = value.chars().count() <= MAX_EMAIL_CHARS
        && !value.chars().any(|c| c.is_whitespace() || c.is_control())
        && value.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.contains('@')
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        });
    if !valid {
        return Err(CrmError::field("email", "is not a valid email address"));
    }
    Ok(value.to_owned())
}

/// Bounded free text; `multiline` allows line breaks and tabs.
fn text(value: &str, field: &'static str, max: usize, multiline: bool) -> Result<String> {
    let bad_control = value
        .chars()
        .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\t')));
    if value.chars().count() > max || bad_control {
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

    fn base() -> serde_json::Value {
        json!({
            "v": 1,
            "type": "customer",
            "name": "  Иванова Мария ",
            "phone": " +7 (912) 000-11-22 ",
            "email": " maria@example.ru ",
            "inn": "500100732259",
            "address": "г. Пермь, ул. Ленина, 1",
            "note": "Любит кефир\nзвонить после 18:00"
        })
    }

    #[test]
    fn valid_party_is_normalized() {
        let party = PartyPayload::parse(&base().to_string()).unwrap();
        assert_eq!(party.party_type, PartyType::Customer);
        assert_eq!(party.name, "Иванова Мария");
        assert_eq!(party.phone.as_deref(), Some("+7 (912) 000-11-22"));
        assert_eq!(party.email.as_deref(), Some("maria@example.ru"));
        assert_eq!(party.inn.as_deref(), Some("500100732259"));
        assert!(party.note.unwrap().contains('\n'));
        assert!(!party.archived);
    }

    #[test]
    fn blank_optional_fields_become_none() {
        let mut value = base();
        for field in ["phone", "email", "inn", "address", "note"] {
            value[field] = json!("  ");
        }
        let party = PartyPayload::parse(&value.to_string()).unwrap();
        assert_eq!(
            (
                party.phone,
                party.email,
                party.inn,
                party.address,
                party.note
            ),
            (None, None, None, None, None)
        );
    }

    #[test]
    fn invalid_fields_are_rejected() {
        let cases: [(&str, serde_json::Value); 9] = [
            ("name", json!("")),
            ("type", json!("partner")),
            ("phone", json!("12")),
            ("phone", json!("call me")),
            ("email", json!("maria@")),
            ("email", json!("ma ria@example.ru")),
            ("inn", json!("500100732258")),
            ("address", json!("a\nb")),
            ("note", json!("x".repeat(MAX_NOTE_CHARS + 1))),
        ];
        for (field, value) in cases {
            let mut payload = base();
            payload[field] = value.clone();
            assert!(
                PartyPayload::parse(&payload.to_string()).is_err(),
                "{field} = {value} should be rejected"
            );
        }
    }

    #[test]
    fn unknown_fields_and_versions_are_rejected() {
        let mut value = base();
        value["discount"] = json!(5);
        assert!(PartyPayload::parse(&value.to_string()).is_err());
        let mut value = base();
        value["v"] = json!(2);
        assert_eq!(
            PartyPayload::parse(&value.to_string()),
            Err(CrmError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn phone_digits_strip_formatting() {
        assert_eq!(phone_digits("+7 (912) 000-11-22"), "79120001122");
    }

    #[test]
    fn only_customers_buy() {
        assert!(PartyType::Customer.buys() && PartyType::Both.buys());
        assert!(!PartyType::Supplier.buys());
        for party_type in PartyType::ALL {
            let json = serde_json::to_string(&party_type).unwrap();
            assert_eq!(json, format!("\"{}\"", party_type.as_str()));
        }
    }
}
