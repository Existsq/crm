//! Guards that keep CRM history and the CRM space channel intact.
//!
//! CRM events are accounting records: corrections are new events (revision,
//! void, return), never deletions. The space channel must stay private,
//! permanent and present while it hosts the CRM (ADR-0002).

use std::sync::Arc;

use nostr::Event;
use uuid::Uuid;

use buzz_core::kind::{
    is_crm_kind, KIND_DELETION, KIND_NIP29_DELETE_EVENT, KIND_NIP29_DELETE_GROUP,
    KIND_NIP29_EDIT_METADATA,
};
use buzz_core::tenant::TenantContext;

use super::db_error;
use crate::handlers::ingest::IngestError;
use crate::state::AppState;

/// Whether `kind` is one of the admin/deletion kinds these guards inspect.
pub(crate) fn applies_to(kind: u32) -> bool {
    matches!(
        kind,
        KIND_DELETION
            | KIND_NIP29_DELETE_EVENT
            | KIND_NIP29_EDIT_METADATA
            | KIND_NIP29_DELETE_GROUP
    )
}

/// Reject deletions of CRM events and destructive edits of the space channel.
pub(crate) async fn validate(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    kind: u32,
    event: &Event,
    channel_id: Option<Uuid>,
) -> Result<(), IngestError> {
    match kind {
        KIND_DELETION | KIND_NIP29_DELETE_EVENT => reject_crm_targets(tenant, state, event).await,
        KIND_NIP29_EDIT_METADATA | KIND_NIP29_DELETE_GROUP => {
            let Some(channel_id) = channel_id else {
                return Ok(());
            };
            let is_space = state
                .db
                .crm_is_space_channel(tenant.community(), channel_id)
                .await
                .map_err(|e| db_error("check CRM space channel", e))?;
            if is_space {
                space_channel_edit(kind, event)
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

async fn reject_crm_targets(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
) -> Result<(), IngestError> {
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        match parts.first().map(String::as_str) {
            Some("e") => {
                let Some(target) = parts
                    .get(1)
                    .and_then(|value| hex::decode(value).ok())
                    .filter(|bytes| bytes.len() == 32)
                else {
                    continue;
                };
                let stored = state
                    .db
                    .get_event_by_id_including_deleted_for_event_write(tenant.community(), &target)
                    .await
                    .map_err(|e| db_error("look up deletion target", e))?;
                if let Some(stored) = stored {
                    if is_crm_kind(u32::from(stored.event.kind.as_u16())) {
                        return Err(crm_history_is_immutable());
                    }
                }
            }
            Some("a") => {
                let target_kind = parts
                    .get(1)
                    .and_then(|coordinate| coordinate.split(':').next())
                    .and_then(|kind| kind.parse::<u32>().ok());
                if target_kind.is_some_and(is_crm_kind) {
                    return Err(crm_history_is_immutable());
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn crm_history_is_immutable() -> IngestError {
    IngestError::Rejected(
        "invalid: CRM events cannot be deleted; post a correcting CRM event instead".into(),
    )
}

/// Pure decision for edits of the active CRM space channel.
fn space_channel_edit(kind: u32, event: &Event) -> Result<(), IngestError> {
    if kind == KIND_NIP29_DELETE_GROUP {
        return Err(IngestError::Rejected(
            "invalid: the CRM space channel cannot be deleted".into(),
        ));
    }
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        match (
            parts.first().map(String::as_str),
            parts.get(1).map(String::as_str),
        ) {
            (Some("visibility"), Some(value)) if value != "private" => {
                return Err(IngestError::Rejected(
                    "invalid: the CRM space channel must stay private".into(),
                ));
            }
            (Some("ttl"), Some(value)) if !value.is_empty() => {
                return Err(IngestError::Rejected(
                    "invalid: the CRM space channel cannot expire".into(),
                ));
            }
            (Some("archived"), Some("true")) => {
                return Err(IngestError::Rejected(
                    "invalid: the CRM space channel cannot be archived".into(),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    fn event(kind: u32, tags: &[&[&str]]) -> Event {
        EventBuilder::new(Kind::Custom(kind as u16), "")
            .tags(tags.iter().map(|t| Tag::parse(t.iter().copied()).unwrap()))
            .sign_with_keys(&Keys::generate())
            .unwrap()
    }

    fn rejected(result: Result<(), IngestError>) -> Option<String> {
        match result {
            Ok(()) => None,
            Err(IngestError::Rejected(m)) => Some(m),
            Err(_) => Some("<other error>".into()),
        }
    }

    #[test]
    fn space_channel_metadata_edits() {
        let h = ["h", "00000000-0000-0000-0000-000000000001"];
        let cases: [(&[&[&str]], Option<&str>); 8] = [
            (&[&h, &["name", "касса"]], None),
            (&[&h, &["about", "retail"]], None),
            (&[&h, &["visibility", "private"]], None),
            (&[&h, &["ttl", ""]], None),
            (&[&h, &["archived", "false"]], None),
            (
                &[&h, &["visibility", "open"]],
                Some("invalid: the CRM space channel must stay private"),
            ),
            (
                &[&h, &["ttl", "3600"]],
                Some("invalid: the CRM space channel cannot expire"),
            ),
            (
                &[&h, &["archived", "true"]],
                Some("invalid: the CRM space channel cannot be archived"),
            ),
        ];
        for (tags, expected) in cases {
            let ev = event(KIND_NIP29_EDIT_METADATA, tags);
            assert_eq!(
                rejected(space_channel_edit(KIND_NIP29_EDIT_METADATA, &ev)).as_deref(),
                expected,
                "{tags:?}"
            );
        }
        let delete = event(KIND_NIP29_DELETE_GROUP, &[&h]);
        assert_eq!(
            rejected(space_channel_edit(KIND_NIP29_DELETE_GROUP, &delete)).as_deref(),
            Some("invalid: the CRM space channel cannot be deleted")
        );
    }

    #[test]
    fn crm_kinds_never_trigger_push_delivery() {
        // CRM records are data, not conversation: they must not wake phones.
        for kind in crate::handlers::push_lease::PUSH_KINDS {
            let kind = u32::try_from(*kind).expect("push kinds fit u32");
            assert!(!is_crm_kind(kind), "CRM kind {kind} in PUSH_KINDS");
        }
    }

    #[test]
    fn guard_scope_is_limited_to_deletion_and_channel_admin_kinds() {
        for kind in [5, 9005, 9002, 9008] {
            assert!(applies_to(kind));
        }
        for kind in [0, 7, 9, 9000, 9001, 9007, 40002, 42100] {
            assert!(!applies_to(kind));
        }
    }
}
