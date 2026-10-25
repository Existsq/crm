//! Reminder rules (kind 42150) and customers' Telegram links (kind 42151).

use nostr::Event;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use buzz_core::tenant::TenantContext;
use buzz_crm::{tags, ClientTelegramPayload, CrmAction, ReminderSettingsPayload};
use buzz_db::crm::{self as store, ClientTelegramOutcome, ReminderSettingsOutcome};

use super::db_error;
use super::space::{conflict, invalid, require_live_space, tag_slices};
use crate::handlers::ingest::IngestError;

/// A kind 42150 event whose payload and tags passed pure validation.
pub(super) struct PreparedReminderSettings {
    payload: ReminderSettingsPayload,
    expected_revision: Option<[u8; 32]>,
}

impl PreparedReminderSettings {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        Ok(Self {
            payload: ReminderSettingsPayload::parse(&event.content).map_err(invalid)?,
            expected_revision: tags::expected_revision(tag_slices(event)).map_err(invalid)?,
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
        let settings = serde_json::to_value(&self.payload)
            .map_err(|e| IngestError::Internal(format!("error: encode reminder settings: {e}")))?;
        let write = store::ReminderSettingsWrite {
            space_channel_id: channel_id,
            settings: &settings,
            enabled: self.payload.enabled,
            revision_event_id: event.id.as_bytes(),
            actor: &actor,
        };
        let outcome = store::reminders::write_reminder_settings(
            tx,
            tenant.community(),
            write,
            self.expected_revision.as_ref().map(|r| r.as_slice()),
        )
        .await
        .map_err(|e| db_error("write CRM reminder settings", e))?;
        settings_outcome(outcome)
    }
}

fn settings_outcome(outcome: ReminderSettingsOutcome) -> Result<(), IngestError> {
    match outcome {
        ReminderSettingsOutcome::Written => Ok(()),
        ReminderSettingsOutcome::AlreadyExists => Err(conflict(
            "reminder settings already exist; send expected-revision to update them",
        )),
        ReminderSettingsOutcome::Missing => Err(conflict("reminder settings do not exist yet")),
        ReminderSettingsOutcome::RevisionMismatch => {
            Err(conflict("reminder settings changed since they were loaded"))
        }
    }
}

/// A kind 42151 event whose payload passed pure validation.
pub(super) struct PreparedClientTelegram {
    payload: ClientTelegramPayload,
}

impl PreparedClientTelegram {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        Ok(Self {
            payload: ClientTelegramPayload::parse(&event.content).map_err(invalid)?,
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
            CrmAction::EditParties,
            false,
        )
        .await?;
        let write = store::ClientTelegramWrite {
            space_channel_id: channel_id,
            customer_id: self.payload.customer_id,
            chat_id: self.payload.chat_id,
            opted_out: self.payload.opted_out,
            revision_event_id: event.id.as_bytes(),
        };
        match store::reminders::write_client_telegram(tx, tenant.community(), write)
            .await
            .map_err(|e| db_error("write CRM client Telegram link", e))?
        {
            ClientTelegramOutcome::Written => Ok(()),
            ClientTelegramOutcome::UnknownCustomer => Err(IngestError::Rejected(
                "invalid: customer_id is not a customer of this space".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_settings_are_conflicts() {
        for outcome in [
            ReminderSettingsOutcome::AlreadyExists,
            ReminderSettingsOutcome::Missing,
            ReminderSettingsOutcome::RevisionMismatch,
        ] {
            assert!(matches!(
                settings_outcome(outcome),
                Err(IngestError::CanvasConflict(m)) if m.starts_with("conflict: ")
            ));
        }
        assert!(settings_outcome(ReminderSettingsOutcome::Written).is_ok());
    }
}
