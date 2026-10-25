//! CRM event builders (kinds 42100–42199, see `docs/nips/NIP-CRM.md`).
//!
//! Payloads are built from `buzz_crm` types and validated with the same code
//! the relay runs, so a builder never produces an event the relay would reject
//! for shape reasons. Authorization and CAS checks still happen at the relay.

use buzz_core::kind::{
    KIND_CRM_CASH_SHIFT, KIND_CRM_FISCAL_RECEIPT, KIND_CRM_PARTY, KIND_CRM_PRODUCT, KIND_CRM_SALE,
    KIND_CRM_SALE_VOID, KIND_CRM_SPACE, KIND_CRM_WAREHOUSE,
};
use buzz_crm::{
    space::PAYLOAD_VERSION, tags, CashShiftPayload, FiscalReceiptPayload, PartyPayload,
    ProductPayload, SalePayload, SaleVoidPayload, SpacePayload, SpaceSettings, WarehousePayload,
    WarehouseSeed,
};
use nostr::{EventBuilder, Kind, Tag};
use uuid::Uuid;

use crate::SdkError;

fn tag(parts: &[&str]) -> Result<Tag, SdkError> {
    Tag::parse(parts.iter().copied()).map_err(|e| SdkError::InvalidTag(e.to_string()))
}

fn invalid(error: buzz_crm::CrmError) -> SdkError {
    SdkError::InvalidInput(error.to_string())
}

fn revision_tag(expected_revision: &str) -> Result<Tag, SdkError> {
    tags::parse_event_id(expected_revision).map_err(invalid)?;
    tag(&[tags::TAG_EXPECTED_REVISION, expected_revision])
}

/// Build the kind 42100 event that binds `channel_id` as the CRM space.
///
/// The relay accepts it only from a community owner/admin who is owner/admin
/// of the private stream channel, and only when the community has no space.
pub fn build_crm_space_create(
    channel_id: Uuid,
    settings: SpaceSettings,
    default_warehouse: WarehouseSeed,
) -> Result<EventBuilder, SdkError> {
    let payload = SpacePayload {
        v: PAYLOAD_VERSION,
        settings,
        default_warehouse: Some(default_warehouse),
    };
    let content =
        serde_json::to_string(&payload).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    SpacePayload::parse(&content, false).map_err(invalid)?;
    Ok(
        EventBuilder::new(Kind::Custom(KIND_CRM_SPACE as u16), content)
            .tags([tag(&["h", &channel_id.to_string()])?]),
    )
}

/// Build the kind 42100 event that replaces the space settings.
///
/// `expected_revision` is the `revision` returned by `GET /crm/space`.
pub fn build_crm_space_update(
    channel_id: Uuid,
    settings: SpaceSettings,
    expected_revision: &str,
) -> Result<EventBuilder, SdkError> {
    let payload = SpacePayload {
        v: PAYLOAD_VERSION,
        settings,
        default_warehouse: None,
    };
    let content =
        serde_json::to_string(&payload).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    SpacePayload::parse(&content, true).map_err(invalid)?;
    Ok(
        EventBuilder::new(Kind::Custom(KIND_CRM_SPACE as u16), content).tags([
            tag(&["h", &channel_id.to_string()])?,
            revision_tag(expected_revision)?,
        ]),
    )
}

/// Build a kind 42101 warehouse event: a create when `expected_revision` is
/// `None`, otherwise an update of the warehouse's current revision.
pub fn build_crm_warehouse(
    channel_id: Uuid,
    warehouse_id: Uuid,
    name: &str,
    is_default: bool,
    archived: bool,
    expected_revision: Option<&str>,
) -> Result<EventBuilder, SdkError> {
    let payload = WarehousePayload {
        v: PAYLOAD_VERSION,
        name: name.to_owned(),
        is_default,
        archived,
    };
    let content =
        serde_json::to_string(&payload).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    WarehousePayload::parse(&content).map_err(invalid)?;
    let mut event_tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&[tags::TAG_CRM_ID, &warehouse_id.to_string()])?,
    ];
    if let Some(revision) = expected_revision {
        event_tags.push(revision_tag(revision)?);
    }
    Ok(EventBuilder::new(Kind::Custom(KIND_CRM_WAREHOUSE as u16), content).tags(event_tags))
}

/// Build a kind 42102 product event: a create when `expected_revision` is
/// `None`, otherwise an update of the product's current revision.
///
/// The payload's `v` is set to the current version and the content is
/// validated (and normalized) with the relay's own rules before signing.
pub fn build_crm_product(
    channel_id: Uuid,
    product_id: Uuid,
    mut payload: ProductPayload,
    expected_revision: Option<&str>,
) -> Result<EventBuilder, SdkError> {
    payload.v = PAYLOAD_VERSION;
    let raw = serde_json::to_string(&payload).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    let normalized = ProductPayload::parse(&raw).map_err(invalid)?;
    let content =
        serde_json::to_string(&normalized).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    let mut event_tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&[tags::TAG_CRM_ID, &product_id.to_string()])?,
    ];
    if let Some(revision) = expected_revision {
        event_tags.push(revision_tag(revision)?);
    }
    Ok(EventBuilder::new(Kind::Custom(KIND_CRM_PRODUCT as u16), content).tags(event_tags))
}

/// Build a kind 42103 customer/supplier event: a create when
/// `expected_revision` is `None`, otherwise an update of the party's current
/// revision. The content is validated and normalized with the relay's rules.
pub fn build_crm_party(
    channel_id: Uuid,
    party_id: Uuid,
    mut payload: PartyPayload,
    expected_revision: Option<&str>,
) -> Result<EventBuilder, SdkError> {
    payload.v = PAYLOAD_VERSION;
    let raw = serde_json::to_string(&payload).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    let normalized = PartyPayload::parse(&raw).map_err(invalid)?;
    let content =
        serde_json::to_string(&normalized).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    let mut event_tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&[tags::TAG_CRM_ID, &party_id.to_string()])?,
    ];
    if let Some(revision) = expected_revision {
        event_tags.push(revision_tag(revision)?);
    }
    Ok(EventBuilder::new(Kind::Custom(KIND_CRM_PARTY as u16), content).tags(event_tags))
}

/// Build the kind 42104 event that posts sale `sale_id`.
///
/// Sales are posted once; pick `sale_id` once per sale so a retried submit
/// is rejected as a duplicate instead of selling twice.
pub fn build_crm_sale(
    channel_id: Uuid,
    sale_id: Uuid,
    mut payload: SalePayload,
) -> Result<EventBuilder, SdkError> {
    payload.v = PAYLOAD_VERSION;
    let raw = serde_json::to_string(&payload).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    let normalized = SalePayload::parse(&raw).map_err(invalid)?;
    let content =
        serde_json::to_string(&normalized).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    Ok(
        EventBuilder::new(Kind::Custom(KIND_CRM_SALE as u16), content).tags([
            tag(&["h", &channel_id.to_string()])?,
            tag(&[tags::TAG_CRM_ID, &sale_id.to_string()])?,
        ]),
    )
}

/// Build the kind 42105 event that voids (сторно) sale `sale_id`.
pub fn build_crm_sale_void(
    channel_id: Uuid,
    sale_id: Uuid,
    reason: &str,
) -> Result<EventBuilder, SdkError> {
    let payload = SaleVoidPayload {
        v: PAYLOAD_VERSION,
        reason: reason.to_owned(),
    };
    let raw = serde_json::to_string(&payload).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    let normalized = SaleVoidPayload::parse(&raw).map_err(invalid)?;
    let content =
        serde_json::to_string(&normalized).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    Ok(
        EventBuilder::new(Kind::Custom(KIND_CRM_SALE_VOID as u16), content).tags([
            tag(&["h", &channel_id.to_string()])?,
            tag(&[tags::TAG_CRM_ID, &sale_id.to_string()])?,
        ]),
    )
}

/// Build the kind 42120 event recording the receipt the ККМ printed for
/// `sale_id` (or its return).
pub fn build_crm_fiscal_receipt(
    channel_id: Uuid,
    sale_id: Uuid,
    mut payload: FiscalReceiptPayload,
) -> Result<EventBuilder, SdkError> {
    payload.v = PAYLOAD_VERSION;
    let content =
        serde_json::to_string(&payload).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    FiscalReceiptPayload::parse(&content).map_err(invalid)?;
    Ok(
        EventBuilder::new(Kind::Custom(KIND_CRM_FISCAL_RECEIPT as u16), content).tags([
            tag(&["h", &channel_id.to_string()])?,
            tag(&[tags::TAG_CRM_ID, &sale_id.to_string()])?,
        ]),
    )
}

/// Build the kind 42121 event recording a shift opened or closed on a ККМ.
pub fn build_crm_cash_shift(
    channel_id: Uuid,
    mut payload: CashShiftPayload,
) -> Result<EventBuilder, SdkError> {
    payload.v = PAYLOAD_VERSION;
    let content =
        serde_json::to_string(&payload).map_err(|e| SdkError::InvalidInput(e.to_string()))?;
    CashShiftPayload::parse(&content).map_err(invalid)?;
    Ok(
        EventBuilder::new(Kind::Custom(KIND_CRM_CASH_SHIFT as u16), content)
            .tags([tag(&["h", &channel_id.to_string()])?]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_crm::{Currency, FiscalizationMode, VatRate};
    use nostr::Keys;

    fn settings() -> SpaceSettings {
        SpaceSettings {
            currency: Currency::Rub,
            timezone: "Europe/Moscow".into(),
            allow_negative_stock: false,
            fiscalization: FiscalizationMode::Required,
            default_vat: VatRate::Vat22,
        }
    }

    fn tag_values(event: &nostr::Event, name: &str) -> Vec<String> {
        event
            .tags
            .iter()
            .filter(|t| t.as_slice().first().map(String::as_str) == Some(name))
            .filter_map(|t| t.as_slice().get(1).cloned())
            .collect()
    }

    #[test]
    fn space_create_round_trips_through_relay_validation() {
        let keys = Keys::generate();
        let channel = Uuid::new_v4();
        let warehouse = Uuid::new_v4();
        let event = build_crm_space_create(
            channel,
            settings(),
            WarehouseSeed {
                id: warehouse,
                name: "Основной склад".into(),
            },
        )
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
        assert_eq!(u32::from(event.kind.as_u16()), KIND_CRM_SPACE);
        assert_eq!(tag_values(&event, "h"), vec![channel.to_string()]);
        let parsed = SpacePayload::parse(&event.content, false).unwrap();
        assert_eq!(parsed.default_warehouse.unwrap().id, warehouse);
    }

    #[test]
    fn space_update_requires_hex_revision() {
        let channel = Uuid::new_v4();
        assert!(build_crm_space_update(channel, settings(), "not-hex").is_err());
        let event = build_crm_space_update(channel, settings(), &"ab".repeat(32))
            .unwrap()
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert_eq!(
            tag_values(&event, "expected-revision"),
            vec!["ab".repeat(32)]
        );
        assert!(SpacePayload::parse(&event.content, true).is_ok());
    }

    fn product(name: &str) -> ProductPayload {
        ProductPayload {
            v: 0,
            name: name.into(),
            sku: Some(" SKU-1 ".into()),
            barcodes: vec!["4006381333931".into()],
            unit: buzz_crm::Unit::Piece,
            price_minor: buzz_crm::Money::from_minor(12_345).unwrap(),
            vat: VatRate::Vat22,
            payment_object: buzz_crm::PaymentObject::Commodity,
            category: None,
            min_stock_milli: buzz_crm::Qty::ZERO,
            requires_marking: false,
            archived: false,
        }
    }

    #[test]
    fn product_builder_normalizes_validates_and_tags() {
        let channel = Uuid::new_v4();
        let id = Uuid::new_v4();
        assert!(build_crm_product(channel, id, product("  "), None).is_err());
        let event = build_crm_product(channel, id, product(" Чай "), Some(&"cd".repeat(32)))
            .unwrap()
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert_eq!(u32::from(event.kind.as_u16()), KIND_CRM_PRODUCT);
        assert_eq!(tag_values(&event, "crm-id"), vec![id.to_string()]);
        assert_eq!(
            tag_values(&event, "expected-revision"),
            vec!["cd".repeat(32)]
        );
        let parsed = ProductPayload::parse(&event.content).unwrap();
        assert_eq!(parsed.v, PAYLOAD_VERSION);
        assert_eq!(parsed.name, "Чай");
        assert_eq!(parsed.sku.as_deref(), Some("SKU-1"));
    }

    #[test]
    fn warehouse_builder_validates_and_tags() {
        let channel = Uuid::new_v4();
        let id = Uuid::new_v4();
        assert!(build_crm_warehouse(channel, id, "   ", false, false, None).is_err());
        assert!(build_crm_warehouse(channel, id, "A", true, true, None).is_err());
        let event = build_crm_warehouse(channel, id, "Магазин", false, false, None)
            .unwrap()
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert_eq!(tag_values(&event, "crm-id"), vec![id.to_string()]);
        assert!(tag_values(&event, "expected-revision").is_empty());
        assert_eq!(u32::from(event.kind.as_u16()), KIND_CRM_WAREHOUSE);
    }
    #[test]
    fn party_builder_normalizes_and_tags() {
        let channel = Uuid::new_v4();
        let id = Uuid::new_v4();
        let party = |name: &str| PartyPayload {
            v: 0,
            party_type: buzz_crm::PartyType::Customer,
            name: name.into(),
            phone: Some(" +7 912 000-11-22 ".into()),
            email: None,
            inn: None,
            address: None,
            note: None,
            archived: false,
        };
        assert!(build_crm_party(channel, id, party(" "), None).is_err());
        let event = build_crm_party(channel, id, party(" Мария "), None)
            .unwrap()
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert_eq!(u32::from(event.kind.as_u16()), KIND_CRM_PARTY);
        assert_eq!(tag_values(&event, "crm-id"), vec![id.to_string()]);
        assert!(tag_values(&event, "expected-revision").is_empty());
        let parsed = PartyPayload::parse(&event.content).unwrap();
        assert_eq!(parsed.name, "Мария");
        assert_eq!(parsed.phone.as_deref(), Some("+7 912 000-11-22"));
    }

    #[test]
    fn sale_and_void_builders_validate_and_tag() {
        let channel = Uuid::new_v4();
        let sale_id = Uuid::new_v4();
        let sale = |qty: i64| SalePayload {
            v: 0,
            warehouse_id: Uuid::new_v4(),
            customer_id: None,
            payment: buzz_crm::PaymentMethod::Cash,
            payments: Vec::new(),
            cash_tendered_minor: None,
            lines: vec![buzz_crm::SaleLine {
                product_id: Uuid::new_v4(),
                qty_milli: buzz_crm::Qty::from_milli(qty).unwrap(),
                price_minor: buzz_crm::Money::from_minor(100).unwrap(),
                discount_minor: buzz_crm::Money::ZERO,
            }],
            note: None,
        };
        assert!(build_crm_sale(channel, sale_id, sale(0)).is_err());
        let event = build_crm_sale(channel, sale_id, sale(2000))
            .unwrap()
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert_eq!(u32::from(event.kind.as_u16()), KIND_CRM_SALE);
        assert_eq!(tag_values(&event, "crm-id"), vec![sale_id.to_string()]);
        assert_eq!(
            SalePayload::parse(&event.content)
                .unwrap()
                .total()
                .unwrap()
                .minor(),
            200
        );

        assert!(build_crm_sale_void(channel, sale_id, "  ").is_err());
        let void = build_crm_sale_void(channel, sale_id, "ошибка")
            .unwrap()
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert_eq!(u32::from(void.kind.as_u16()), KIND_CRM_SALE_VOID);
        assert_eq!(tag_values(&void, "crm-id"), vec![sale_id.to_string()]);
    }

    #[test]
    fn fiscal_builders_validate_and_tag() {
        let channel = Uuid::new_v4();
        let sale = Uuid::new_v4();
        let receipt = FiscalReceiptPayload {
            v: 0,
            operation: buzz_crm::FiscalOperation::Sell,
            total_minor: buzz_crm::Money::from_minor(100).unwrap(),
            task_uuid: Uuid::new_v4().to_string(),
            fiscal: buzz_crm::FiscalParams {
                document_number: 1,
                document_sign: "123".into(),
                receipt_number: 1,
                shift_number: 1,
                fn_number: "9999".into(),
                registration_number: None,
                document_datetime: "2026-10-25T10:00:00+03:00".into(),
            },
        };
        let event = build_crm_fiscal_receipt(channel, sale, receipt.clone())
            .unwrap()
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert_eq!(u32::from(event.kind.as_u16()), KIND_CRM_FISCAL_RECEIPT);
        assert_eq!(tag_values(&event, "crm-id"), vec![sale.to_string()]);
        let mut bad = receipt;
        bad.task_uuid = "x".into();
        assert!(build_crm_fiscal_receipt(channel, sale, bad).is_err());

        let shift = CashShiftPayload {
            v: 0,
            action: buzz_crm::ShiftAction::Open,
            fn_number: "9999".into(),
            shift_number: 1,
            document_number: 1,
            document_datetime: "2026-10-25T09:00:00+03:00".into(),
        };
        let event = build_crm_cash_shift(channel, shift)
            .unwrap()
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert_eq!(u32::from(event.kind.as_u16()), KIND_CRM_CASH_SHIFT);
    }
}
