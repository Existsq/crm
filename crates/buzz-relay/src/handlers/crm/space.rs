//! Space settings (kind 42100) and warehouses (kind 42101).

use nostr::Event;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use buzz_core::tenant::TenantContext;
use buzz_crm::{tags, CrmAction, CrmError, SpacePayload, WarehousePayload};
use buzz_db::crm::{self as store, CasOutcome, SpaceCreateOutcome, WarehouseWriteOutcome};

use super::db_error;
use super::decide;
use crate::handlers::ingest::IngestError;

/// A CRM event whose payload and tags passed pure validation.
pub(super) enum Prepared {
    /// Kind 42100 without `expected-revision`: bind the channel as the space.
    CreateSpace(SpacePayload),
    /// Kind 42100 with `expected-revision`: replace the settings.
    UpdateSpace {
        payload: SpacePayload,
        expected_revision: [u8; 32],
    },
    /// Kind 42101: create or update a warehouse.
    Warehouse {
        warehouse_id: Uuid,
        payload: WarehousePayload,
        expected_revision: Option<[u8; 32]>,
    },
}

fn invalid(error: CrmError) -> IngestError {
    IngestError::Rejected(format!("invalid: {error}"))
}

fn tag_slices(event: &Event) -> impl Iterator<Item = &[String]> {
    event.tags.iter().map(|tag| tag.as_slice())
}

fn conflict(message: &str) -> IngestError {
    // Optimistic-concurrency failures share the canvas CAS variant so the HTTP
    // bridge answers 409 and the CLI exits with its write-conflict code.
    IngestError::CanvasConflict(format!("conflict: {message}"))
}

impl Prepared {
    pub(super) fn space(event: &Event) -> Result<Self, IngestError> {
        let expected_revision = tags::expected_revision(tag_slices(event)).map_err(invalid)?;
        let payload =
            SpacePayload::parse(&event.content, expected_revision.is_some()).map_err(invalid)?;
        Ok(match expected_revision {
            None => Prepared::CreateSpace(payload),
            Some(expected_revision) => Prepared::UpdateSpace {
                payload,
                expected_revision,
            },
        })
    }

    pub(super) fn warehouse(event: &Event) -> Result<Self, IngestError> {
        let warehouse_id = tags::crm_id(tag_slices(event)).map_err(invalid)?;
        let expected_revision = tags::expected_revision(tag_slices(event)).map_err(invalid)?;
        let payload = WarehousePayload::parse(&event.content).map_err(invalid)?;
        Ok(Prepared::Warehouse {
            warehouse_id,
            payload,
            expected_revision,
        })
    }

    pub(super) async fn apply(
        self,
        tx: &mut Transaction<'static, Postgres>,
        tenant: &TenantContext,
        event: &Event,
        channel_id: Uuid,
    ) -> Result<(), IngestError> {
        let community = tenant.community();
        let actor = event.pubkey.to_bytes();
        let revision = event.id.as_bytes();

        match self {
            Prepared::CreateSpace(payload) => {
                let relay_role = store::relay_role_for_write(tx, community, &event.pubkey.to_hex())
                    .await
                    .map_err(|e| db_error("read community role", e))?;
                let channel_role = store::channel_role_for_write(tx, community, channel_id, &actor)
                    .await
                    .map_err(|e| db_error("read channel role", e))?;
                decide::may_create_space(relay_role.as_deref(), channel_role.as_deref())?;
                let facts = store::channel_facts_for_write(tx, community, channel_id)
                    .await
                    .map_err(|e| db_error("read channel", e))?;
                decide::channel_can_host_space(facts.as_ref())?;
                check_timezone(tx, &payload).await?;

                let seed = payload
                    .default_warehouse
                    .as_ref()
                    .ok_or_else(|| IngestError::Internal("error: validated seed missing".into()))?;
                let settings = settings_json(&payload)?;
                match store::create_space(
                    tx, community, channel_id, &settings, revision, &actor, seed.id, &seed.name,
                )
                .await
                .map_err(|e| db_error("create CRM space", e))?
                {
                    SpaceCreateOutcome::Created => Ok(()),
                    SpaceCreateOutcome::CommunityHasSpace => {
                        Err(conflict("this community already has a CRM space"))
                    }
                    SpaceCreateOutcome::WarehouseIdTaken => {
                        Err(conflict("default warehouse id is already in use"))
                    }
                }
            }
            Prepared::UpdateSpace {
                payload,
                expected_revision,
            } => {
                require_live_space(tx, tenant, channel_id, &actor, CrmAction::ManageSpace, true)
                    .await?;
                check_timezone(tx, &payload).await?;
                let settings = settings_json(&payload)?;
                match store::update_space_settings(
                    tx,
                    community,
                    channel_id,
                    &settings,
                    &expected_revision,
                    revision,
                )
                .await
                .map_err(|e| db_error("update CRM space", e))?
                {
                    CasOutcome::Updated => Ok(()),
                    CasOutcome::Missing => Err(conflict("CRM space revision does not exist")),
                    CasOutcome::RevisionMismatch => Err(conflict(
                        "CRM space settings changed since they were loaded",
                    )),
                }
            }
            Prepared::Warehouse {
                warehouse_id,
                payload,
                expected_revision,
            } => {
                require_live_space(
                    tx,
                    tenant,
                    channel_id,
                    &actor,
                    CrmAction::ManageWarehouses,
                    // Exclusive: serializes warehouse writes per space so two
                    // concurrent default moves cannot both clear-then-set.
                    true,
                )
                .await?;
                let write = store::WarehouseWrite {
                    warehouse_id,
                    space_channel_id: channel_id,
                    name: &payload.name,
                    is_default: payload.is_default,
                    archived: payload.archived,
                    revision_event_id: revision,
                };
                match store::write_warehouse(
                    tx,
                    community,
                    write,
                    expected_revision.as_ref().map(|r| r.as_slice()),
                )
                .await
                .map_err(|e| db_error("write CRM warehouse", e))?
                {
                    WarehouseWriteOutcome::Written => Ok(()),
                    WarehouseWriteOutcome::AlreadyExists => Err(conflict(
                        "warehouse already exists; send expected-revision to update it",
                    )),
                    WarehouseWriteOutcome::Missing => Err(conflict("warehouse does not exist")),
                    WarehouseWriteOutcome::RevisionMismatch => {
                        Err(conflict("warehouse changed since it was loaded"))
                    }
                    WarehouseWriteOutcome::WouldRemoveDefault => Err(IngestError::Rejected(
                        "invalid: make another warehouse the default first".into(),
                    )),
                }
            }
        }
    }
}

/// Lock the space row for `channel_id`, confirm the channel still qualifies,
/// and authorize `action` for the caller's current channel role.
pub(super) async fn require_live_space(
    tx: &mut Transaction<'static, Postgres>,
    tenant: &TenantContext,
    channel_id: Uuid,
    actor: &[u8],
    action: CrmAction,
    exclusive: bool,
) -> Result<(), IngestError> {
    let community = tenant.community();
    store::space_for_write(tx, community, channel_id, exclusive)
        .await
        .map_err(|e| db_error("read CRM space", e))?
        .ok_or_else(|| {
            IngestError::Rejected("invalid: this channel is not the CRM space".into())
        })?;
    let facts = store::channel_facts_for_write(tx, community, channel_id)
        .await
        .map_err(|e| db_error("read channel", e))?;
    decide::channel_can_host_space(facts.as_ref())?;
    let role = store::channel_role_for_write(tx, community, channel_id, actor)
        .await
        .map_err(|e| db_error("read channel role", e))?;
    decide::authorize(role.as_deref(), action)
}

async fn check_timezone(
    tx: &mut Transaction<'static, Postgres>,
    payload: &SpacePayload,
) -> Result<(), IngestError> {
    let known = store::timezone_exists(tx, &payload.settings.timezone)
        .await
        .map_err(|e| db_error("check timezone", e))?;
    if known {
        Ok(())
    } else {
        Err(IngestError::Rejected(format!(
            "invalid: settings.timezone: unknown timezone {:?}",
            payload.settings.timezone
        )))
    }
}

fn settings_json(payload: &SpacePayload) -> Result<serde_json::Value, IngestError> {
    serde_json::to_value(&payload.settings)
        .map_err(|e| IngestError::Internal(format!("error: serialize CRM settings: {e}")))
}
