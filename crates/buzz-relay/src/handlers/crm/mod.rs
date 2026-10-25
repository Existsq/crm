//! CRM ingest branch (kinds 42100–42199).
//!
//! Every CRM event is stored together with its `crm_*` projection rows in one
//! database transaction, after an uncached role check read inside that same
//! transaction (docs/crm/adr/0001, 0002). Callers reach this module only after
//! the generic ingest gates: signature, timestamp window, pubkey/auth match,
//! scope, ban/timeout, `h`-channel resolution, token-channel access, channel
//! membership and the archived-channel check.

mod decide;
pub(crate) mod guards;
mod space;

use std::sync::Arc;

use nostr::Event;
use sqlx::{Postgres, Transaction};
use tracing::info;
use uuid::Uuid;

use buzz_core::kind::{event_kind_u32, KIND_CRM_SPACE, KIND_CRM_WAREHOUSE};
use buzz_core::tenant::TenantContext;

use super::event::dispatch_persistent_event;
use super::ingest::{IngestAuth, IngestError, IngestResult};
use crate::conformance::{
    channel_label, claimed_community_from_event, emit, msg_id_label, state_for_request, TraceAction,
};
use crate::state::{AppState, ThreadedChannelVisibility};

/// Ingest one CRM event: validate, authorize, persist event + projection
/// atomically, then fan out to the space channel.
pub(crate) async fn ingest_crm_event(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    tracer: &Arc<dyn buzz_conformance::Tracer>,
    event: &Event,
    auth: &IngestAuth,
    channel_id: Uuid,
    threaded_visibility: Option<ThreadedChannelVisibility>,
) -> Result<IngestResult, IngestError> {
    let kind = event_kind_u32(event);
    let event_id_hex = event.id.to_hex();

    // Pure validation first: malformed payloads never open a transaction.
    let prepared = match kind {
        KIND_CRM_SPACE => space::Prepared::space(event)?,
        KIND_CRM_WAREHOUSE => space::Prepared::warehouse(event)?,
        other => {
            return Err(IngestError::Rejected(format!(
                "invalid: CRM kind {other} is not supported by this relay"
            )))
        }
    };

    let mut tx = begin(state, tenant).await?;
    let (stored_event, was_inserted) = buzz_db::event::insert_event_in_transaction(
        &mut tx,
        tenant.community(),
        event,
        Some(channel_id),
    )
    .await
    .map_err(|e| IngestError::Internal(format!("error: insert CRM event: {e}")))?;
    if !was_inserted {
        // Identical signed event already stored: its projection was applied by
        // the original write. Nothing to re-apply.
        rollback(tx).await;
        return Ok(IngestResult {
            event_id: event_id_hex,
            accepted: true,
            message: "duplicate:".into(),
        });
    }

    if let Err(error) = prepared.apply(&mut tx, tenant, event, channel_id).await {
        rollback(tx).await;
        return Err(error);
    }
    tx.commit()
        .await
        .map_err(|e| IngestError::Internal(format!("error: commit CRM event: {e}")))?;

    emit(
        tracer,
        TraceAction::WriteInsert {
            msg_id: msg_id_label(event.id.as_bytes()),
            channel: channel_label(channel_id),
            claimed_community: claimed_community_from_event(event),
        },
        state_for_request(tenant, auth.pubkey()),
    );
    let pubkey_hex = auth.pubkey().to_hex();
    dispatch_persistent_event(
        tenant,
        state,
        &stored_event,
        kind,
        &pubkey_hex,
        threaded_visibility,
    )
    .await;

    info!(event_id = %event_id_hex, kind, "CRM event ingested");
    Ok(IngestResult {
        event_id: event_id_hex,
        accepted: true,
        message: String::new(),
    })
}

async fn begin(
    state: &Arc<AppState>,
    tenant: &TenantContext,
) -> Result<Transaction<'static, Postgres>, IngestError> {
    let mut tx = state
        .db
        .begin_event_write_transaction()
        .await
        .map_err(|e| IngestError::Internal(format!("error: begin transaction: {e}")))?;
    buzz_deletion::store(&state.db)
        .guard_transaction(&mut tx, tenant.community())
        .await
        .map_err(|error| {
            IngestError::Rejected(format!("restricted: community writes are fenced: {error}"))
        })?;
    Ok(tx)
}

async fn rollback(tx: Transaction<'static, Postgres>) {
    if let Err(error) = tx.rollback().await {
        // Dropping the connection aborts the transaction server-side anyway;
        // the caller already returns the real rejection reason.
        tracing::warn!(%error, "CRM transaction rollback failed");
    }
}

fn db_error(context: &str, error: buzz_db::DbError) -> IngestError {
    IngestError::Internal(format!("error: {context}: {error}"))
}
