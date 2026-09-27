//! CRM event builders (kinds 42100–42199, see `docs/nips/NIP-CRM.md`).
//!
//! Payloads are built from `buzz_crm` types and validated with the same code
//! the relay runs, so a builder never produces an event the relay would reject
//! for shape reasons. Authorization and CAS checks still happen at the relay.

use buzz_core::kind::{KIND_CRM_SPACE, KIND_CRM_WAREHOUSE};
use buzz_crm::{
    space::PAYLOAD_VERSION, tags, SpacePayload, SpaceSettings, WarehousePayload, WarehouseSeed,
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
}
