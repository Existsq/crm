//! CRM space (kind 42100) and warehouse (kind 42101) payloads.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{CrmError, Result};
use crate::validate;
use crate::vat::VatRate;

/// The only payload schema version this build accepts.
pub const PAYLOAD_VERSION: u64 = 1;

/// Settlement currency. Only Russian rubles are supported in v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Currency {
    /// Russian ruble.
    #[serde(rename = "RUB")]
    Rub,
}

/// How strictly sales must be fiscalized on the cash register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FiscalizationMode {
    /// Every sale and return must get a fiscal receipt.
    Required,
    /// A fiscal receipt may be printed; unfiscalized sales are allowed.
    Optional,
    /// No cash register is used.
    Off,
}

/// Space-wide CRM settings, replaced as a whole on every update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceSettings {
    /// Settlement currency.
    pub currency: Currency,
    /// IANA timezone used for document dates and analytics day boundaries.
    pub timezone: String,
    /// Whether stock may go below zero.
    pub allow_negative_stock: bool,
    /// Fiscalization policy for sales.
    pub fiscalization: FiscalizationMode,
    /// VAT preselected for new products.
    pub default_vat: VatRate,
}

impl SpaceSettings {
    fn validate(&self) -> Result<()> {
        validate::timezone(&self.timezone)?;
        if self.default_vat.is_historical() {
            return Err(CrmError::field(
                "settings.default_vat",
                format!("{} is historical and cannot be a default", self.default_vat),
            ));
        }
        Ok(())
    }
}

/// The warehouse created together with a new space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarehouseSeed {
    /// Client-chosen warehouse UUID.
    pub id: Uuid,
    /// Display name.
    pub name: String,
}

/// Content of a kind 42100 event.
///
/// Creation (no `expected-revision` tag) must carry `default_warehouse`;
/// updates must not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpacePayload {
    /// Schema version, always [`PAYLOAD_VERSION`].
    pub v: u64,
    /// Settings to store.
    pub settings: SpaceSettings,
    /// Warehouse to create with the space (creation only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_warehouse: Option<WarehouseSeed>,
}

impl SpacePayload {
    /// Parse and validate kind 42100 content. `is_update` is true when the
    /// event carries an `expected-revision` tag.
    pub fn parse(content: &str, is_update: bool) -> Result<Self> {
        let mut payload: SpacePayload = parse_versioned(content)?;
        payload.settings.validate()?;
        match (&mut payload.default_warehouse, is_update) {
            (Some(_), true) => {
                return Err(CrmError::field(
                    "default_warehouse",
                    "only allowed when creating the space",
                ))
            }
            (None, false) => {
                return Err(CrmError::field(
                    "default_warehouse",
                    "required when creating the space",
                ))
            }
            (Some(seed), false) => {
                if seed.id.is_nil() {
                    return Err(CrmError::field("default_warehouse.id", "must not be nil"));
                }
                seed.name = validate::name(&seed.name, "default_warehouse.name")?;
            }
            (None, true) => {}
        }
        Ok(payload)
    }
}

/// Content of a kind 42101 event (the warehouse id is the `crm-id` tag).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarehousePayload {
    /// Schema version, always [`PAYLOAD_VERSION`].
    pub v: u64,
    /// Display name.
    pub name: String,
    /// Whether this is the space's default warehouse. Setting it moves the
    /// default here; clearing it on the current default is rejected.
    #[serde(default)]
    pub is_default: bool,
    /// Archived warehouses are hidden from pickers and accept no new stock.
    #[serde(default)]
    pub archived: bool,
}

impl WarehousePayload {
    /// Parse and validate kind 42101 content.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: WarehousePayload = parse_versioned(content)?;
        payload.name = validate::name(&payload.name, "name")?;
        if payload.is_default && payload.archived {
            return Err(CrmError::field(
                "archived",
                "the default warehouse cannot be archived",
            ));
        }
        Ok(payload)
    }
}

/// Parse JSON content whose top-level `v` must equal [`PAYLOAD_VERSION`].
///
/// The version is checked before the typed parse so that a future `v: 2`
/// payload reports "unsupported version" instead of a confusing field error.
pub fn parse_versioned<T: serde::de::DeserializeOwned>(content: &str) -> Result<T> {
    let value: serde_json::Value =
        serde_json::from_str(content).map_err(|e| CrmError::MalformedPayload(e.to_string()))?;
    let version = value
        .get("v")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| CrmError::MalformedPayload("missing numeric field `v`".into()))?;
    if version != PAYLOAD_VERSION {
        return Err(CrmError::UnsupportedVersion(version));
    }
    serde_json::from_value(value).map_err(|e| CrmError::MalformedPayload(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings() -> serde_json::Value {
        json!({
            "currency": "RUB",
            "timezone": "Europe/Moscow",
            "allow_negative_stock": false,
            "fiscalization": "required",
            "default_vat": "vat22"
        })
    }

    #[test]
    fn create_requires_default_warehouse_and_trims_name() {
        let id = Uuid::new_v4();
        let content = json!({
            "v": 1,
            "settings": settings(),
            "default_warehouse": {"id": id, "name": "  Основной склад "}
        })
        .to_string();
        let payload = SpacePayload::parse(&content, false).unwrap();
        assert_eq!(payload.default_warehouse.unwrap().name, "Основной склад");

        let without = json!({"v": 1, "settings": settings()}).to_string();
        assert!(SpacePayload::parse(&without, false).is_err());
        assert!(SpacePayload::parse(&without, true).is_ok());
        assert!(SpacePayload::parse(&content, true).is_err());
    }

    #[test]
    fn settings_are_validated() {
        let mut bad_tz = settings();
        bad_tz["timezone"] = json!("Mars/Olympus Mons");
        let content = json!({"v": 1, "settings": bad_tz}).to_string();
        assert!(SpacePayload::parse(&content, true).is_err());

        let mut historical = settings();
        historical["default_vat"] = json!("vat20");
        let content = json!({"v": 1, "settings": historical}).to_string();
        assert!(SpacePayload::parse(&content, true).is_err());

        let mut unknown = settings();
        unknown["currency"] = json!("USD");
        let content = json!({"v": 1, "settings": unknown}).to_string();
        assert!(SpacePayload::parse(&content, true).is_err());

        let mut extra = settings();
        extra["surprise"] = json!(true);
        let content = json!({"v": 1, "settings": extra}).to_string();
        assert!(SpacePayload::parse(&content, true).is_err());
    }

    #[test]
    fn version_is_checked_first() {
        let content = json!({"v": 2, "anything": true}).to_string();
        assert_eq!(
            SpacePayload::parse(&content, true),
            Err(CrmError::UnsupportedVersion(2))
        );
        assert!(matches!(
            SpacePayload::parse("not json", true),
            Err(CrmError::MalformedPayload(_))
        ));
        assert!(matches!(
            SpacePayload::parse("{}", true),
            Err(CrmError::MalformedPayload(_))
        ));
    }

    #[test]
    fn warehouse_payload_rules() {
        let ok =
            WarehousePayload::parse(&json!({"v": 1, "name": " Магазин "}).to_string()).unwrap();
        assert_eq!(ok.name, "Магазин");
        assert!(!ok.is_default && !ok.archived);
        let archived_default =
            json!({"v": 1, "name": "A", "is_default": true, "archived": true}).to_string();
        assert!(WarehousePayload::parse(&archived_default).is_err());
        assert!(WarehousePayload::parse(&json!({"v": 1, "name": ""}).to_string()).is_err());
    }
}
