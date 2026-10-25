//! Customers and suppliers (kind 42103).

use nostr::Event;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use buzz_core::tenant::TenantContext;
use buzz_crm::{party::phone_digits, tags, CrmAction, PartyPayload};
use buzz_db::crm::{self as store, PartyWriteOutcome};

use super::db_error;
use super::space::{conflict, invalid, require_live_space, tag_slices};
use crate::handlers::ingest::IngestError;

/// A kind 42103 event whose payload and tags passed pure validation.
pub(super) struct PreparedParty {
    party_id: Uuid,
    payload: PartyPayload,
    expected_revision: Option<[u8; 32]>,
}

impl PreparedParty {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let party_id = tags::crm_id(tag_slices(event)).map_err(invalid)?;
        let expected_revision = tags::expected_revision(tag_slices(event)).map_err(invalid)?;
        let payload = PartyPayload::parse(&event.content).map_err(invalid)?;
        Ok(Self {
            party_id,
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
        let payload = &self.payload;
        let digits = payload.phone.as_deref().map(phone_digits);
        let write = store::PartyWrite {
            party_id: self.party_id,
            space_channel_id: channel_id,
            party_type: payload.party_type.as_str(),
            name: &payload.name,
            phone: payload.phone.as_deref(),
            phone_digits: digits.as_deref(),
            email: payload.email.as_deref(),
            inn: payload.inn.as_deref(),
            address: payload.address.as_deref(),
            note: payload.note.as_deref(),
            archived: payload.archived,
            revision_event_id: event.id.as_bytes(),
            actor: &actor,
        };
        let outcome = store::parties::write_party(
            tx,
            tenant.community(),
            write,
            self.expected_revision.as_ref().map(|r| r.as_slice()),
        )
        .await
        .map_err(|e| db_error("write CRM party", e))?;
        party_outcome(outcome)
    }
}

fn party_outcome(outcome: PartyWriteOutcome) -> Result<(), IngestError> {
    match outcome {
        PartyWriteOutcome::Written => Ok(()),
        PartyWriteOutcome::AlreadyExists => Err(conflict(
            "party already exists; send expected-revision to update it",
        )),
        PartyWriteOutcome::Missing => Err(conflict("party does not exist")),
        PartyWriteOutcome::RevisionMismatch => Err(conflict("party changed since it was loaded")),
        PartyWriteOutcome::InnTaken(Some(inn)) => Err(conflict(&format!(
            "INN {inn} is already used by another party"
        ))),
        PartyWriteOutcome::InnTaken(None) => {
            Err(conflict("the INN is already used by another party"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(result: Result<(), IngestError>) -> String {
        match result {
            Ok(()) => String::new(),
            Err(IngestError::CanvasConflict(message)) => message,
            Err(other) => panic!("expected a conflict, got {other:?}"),
        }
    }

    #[test]
    fn duplicates_name_the_inn_and_map_to_conflict() {
        assert_eq!(
            message(party_outcome(PartyWriteOutcome::InnTaken(Some(
                "7707083893".into()
            )))),
            "conflict: INN 7707083893 is already used by another party"
        );
        for outcome in [
            PartyWriteOutcome::AlreadyExists,
            PartyWriteOutcome::Missing,
            PartyWriteOutcome::RevisionMismatch,
            PartyWriteOutcome::InnTaken(None),
        ] {
            assert!(message(party_outcome(outcome)).starts_with("conflict: "));
        }
        assert!(party_outcome(PartyWriteOutcome::Written).is_ok());
    }
}
