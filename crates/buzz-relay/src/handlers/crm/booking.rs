//! Online booking: the page settings (kind 42135) and the bookings clients
//! make on it (kind 42134, signed by the relay).
//!
//! Kind 42134 never arrives from a client: the public booking API checks the
//! client's phone code, builds the appointment and hands the signed event to
//! [`ingest_online_booking`], which stores it with its appointment row in one
//! transaction and spends the code there, so a code books at most once.

use std::sync::Arc;

use chrono::NaiveDate;
use nostr::Event;
use sqlx::{Postgres, Transaction};
use tracing::info;
use uuid::Uuid;

use buzz_core::kind::KIND_CRM_ONLINE_BOOKING;
use buzz_core::tenant::TenantContext;
use buzz_crm::{tags, AppointmentPayload, CrmAction, OnlineBookingSettings};
use buzz_db::crm::{booking as store, OnlineBookingWriteOutcome};

use super::space::{conflict, invalid, require_live_space, require_space_channel, tag_slices};
use super::{begin, db_error, rollback};
use crate::handlers::event::dispatch_persistent_event;
use crate::handlers::ingest::IngestError;
use crate::state::AppState;

/// A kind 42135 event whose payload and tags passed pure validation.
pub(super) struct PreparedSettings {
    settings: OnlineBookingSettings,
    expected_revision: Option<[u8; 32]>,
}

impl PreparedSettings {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let expected_revision = tags::expected_revision(tag_slices(event)).map_err(invalid)?;
        let settings = OnlineBookingSettings::parse(&event.content).map_err(invalid)?;
        Ok(Self {
            settings,
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
        let actor = event.pubkey.to_bytes();
        require_live_space(
            tx,
            tenant,
            channel_id,
            &actor,
            CrmAction::ManageSpace,
            false,
        )
        .await?;
        let json = serde_json::to_value(&self.settings).map_err(|e| {
            IngestError::Internal(format!("error: serialize online booking settings: {e}"))
        })?;
        let outcome = store::write_online_booking(
            tx,
            tenant.community(),
            channel_id,
            self.settings.enabled,
            &json,
            event.id.as_bytes(),
            &actor,
            self.expected_revision.as_ref().map(|r| r.as_slice()),
        )
        .await
        .map_err(|e| db_error("write online booking settings", e))?;
        match outcome {
            OnlineBookingWriteOutcome::Written => Ok(()),
            OnlineBookingWriteOutcome::AlreadyExists => Err(conflict(
                "online booking settings already exist; send expected-revision to update them",
            )),
            OnlineBookingWriteOutcome::Missing => {
                Err(conflict("online booking settings do not exist yet"))
            }
            OnlineBookingWriteOutcome::RevisionMismatch => Err(conflict(
                "online booking settings changed since they were loaded",
            )),
        }
    }
}

/// Clients cannot publish kind 42134; only the relay signs it.
pub(super) fn reject_client_booking() -> IngestError {
    IngestError::Rejected("restricted: online bookings are made on the public booking page".into())
}

/// Why a relay-signed online booking was not stored.
#[derive(Debug)]
pub(crate) enum OnlineBookingError {
    /// The phone code was spent by a parallel booking or expired meanwhile.
    CodeSpent,
    /// The appointment broke a rule (`invalid:`) or lost its slot
    /// (`conflict:`), or the write failed.
    Ingest(IngestError),
}

impl From<IngestError> for OnlineBookingError {
    fn from(error: IngestError) -> Self {
        OnlineBookingError::Ingest(error)
    }
}

/// Store a relay-signed kind 42134 event, its appointment row and the spent
/// phone code atomically, then fan the event out to the space channel.
pub(crate) async fn ingest_online_booking(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
    channel_id: Uuid,
    verification_id: Uuid,
) -> Result<(), OnlineBookingError> {
    let relay = state.relay_keypair.public_key();
    if event.pubkey != relay || u32::from(event.kind.as_u16()) != KIND_CRM_ONLINE_BOOKING {
        return Err(
            IngestError::Internal("error: online booking not signed by the relay".into()).into(),
        );
    }
    let appointment_id = tags::crm_id(tag_slices(event)).map_err(invalid)?;
    let payload = AppointmentPayload::parse(&event.content).map_err(invalid)?;
    let day = NaiveDate::parse_from_str(&payload.date, "%Y-%m-%d")
        .map_err(|_| IngestError::Internal("error: online booking date".into()))?;

    let mut tx = begin(state, tenant).await?;
    let result = async {
        require_space_channel(&mut tx, tenant, channel_id, false).await?;
        let (stored, inserted) = buzz_db::event::insert_event_in_transaction(
            &mut tx,
            tenant.community(),
            event,
            Some(channel_id),
        )
        .await
        .map_err(|e| IngestError::Internal(format!("error: insert CRM event: {e}")))?;
        if !inserted {
            return Err(
                IngestError::Internal("error: online booking event replayed".into()).into(),
            );
        }
        let spent =
            store::consume_code(&mut tx, tenant.community(), verification_id, appointment_id)
                .await
                .map_err(|e| db_error("spend booking code", e))?;
        if !spent {
            return Err(OnlineBookingError::CodeSpent);
        }
        super::appointments::project(
            &mut tx,
            tenant,
            event,
            channel_id,
            appointment_id,
            day,
            &payload,
            None,
        )
        .await?;
        Ok(stored)
    }
    .await;
    let stored = match result {
        Ok(stored) => stored,
        Err(error) => {
            rollback(tx).await;
            return Err(error);
        }
    };
    tx.commit()
        .await
        .map_err(|e| IngestError::Internal(format!("error: commit CRM event: {e}")))?;

    dispatch_persistent_event(
        tenant,
        state,
        &stored,
        KIND_CRM_ONLINE_BOOKING,
        &relay.to_hex(),
        None,
    )
    .await;
    info!(event_id = %event.id.to_hex(), %appointment_id, "online booking stored");
    Ok(())
}
