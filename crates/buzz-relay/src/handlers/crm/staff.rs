//! Staff (kind 42131) and their working days (kind 42132).

use chrono::NaiveDate;
use nostr::Event;
use serde_json::json;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use buzz_core::tenant::TenantContext;
use buzz_crm::{tags, CrmAction, CrmError, SchedulePayload, StaffPayload};
use buzz_db::crm::{self as store, ScheduleWriteOutcome, StaffDayWrite, StaffWriteOutcome};

use super::db_error;
use super::space::{conflict, invalid, require_live_space, tag_slices};
use crate::handlers::ingest::IngestError;

/// A kind 42131 event whose payload and tags passed pure validation.
pub(super) struct PreparedStaff {
    staff_id: Uuid,
    payload: StaffPayload,
    member: Option<Vec<u8>>,
    expected_revision: Option<[u8; 32]>,
}

impl PreparedStaff {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let staff_id = tags::crm_id(tag_slices(event)).map_err(invalid)?;
        let expected_revision = tags::expected_revision(tag_slices(event)).map_err(invalid)?;
        let payload = StaffPayload::parse(&event.content).map_err(invalid)?;
        let member = payload
            .member
            .as_deref()
            .map(|hex| {
                hex::decode(hex).map_err(|_| {
                    invalid(CrmError::field(
                        "member",
                        "must be a 64-character hex pubkey",
                    ))
                })
            })
            .transpose()?;
        Ok(Self {
            staff_id,
            payload,
            member,
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
            CrmAction::EditCatalog,
            false,
        )
        .await?;
        let payload = &self.payload;
        let write = store::StaffWrite {
            staff_id: self.staff_id,
            space_channel_id: channel_id,
            name: &payload.name,
            position: payload.position.as_deref(),
            phone: payload.phone.as_deref(),
            service_ids: &payload.service_ids,
            member_pubkey: self.member.as_deref(),
            archived: payload.archived,
            revision_event_id: event.id.as_bytes(),
            actor: &actor,
        };
        let outcome = store::staff::write_staff(
            tx,
            tenant.community(),
            write,
            self.expected_revision.as_ref().map(|r| r.as_slice()),
        )
        .await
        .map_err(|e| db_error("write CRM staff", e))?;
        staff_outcome(outcome)
    }
}

fn staff_outcome(outcome: StaffWriteOutcome) -> Result<(), IngestError> {
    match outcome {
        StaffWriteOutcome::Written => Ok(()),
        StaffWriteOutcome::AlreadyExists => Err(conflict(
            "staff member already exists; send expected-revision to update it",
        )),
        StaffWriteOutcome::Missing => Err(conflict("staff member does not exist")),
        StaffWriteOutcome::RevisionMismatch => {
            Err(conflict("staff member changed since it was loaded"))
        }
        StaffWriteOutcome::UnknownService(id) => Err(IngestError::Rejected(format!(
            "invalid: service_ids: {id} is not a service of this space"
        ))),
    }
}

/// A kind 42132 event whose payload passed pure validation.
pub(super) struct PreparedSchedule {
    staff_id: Uuid,
    days: Vec<StaffDayWrite>,
}

impl PreparedSchedule {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let payload = SchedulePayload::parse(&event.content).map_err(invalid)?;
        let days = payload
            .days
            .iter()
            .map(|day| {
                let date = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d")
                    .map_err(|_| invalid(CrmError::field("date", "is not a YYYY-MM-DD date")))?;
                let intervals: Vec<_> = day
                    .intervals
                    .iter()
                    .map(|interval| json!({"start": interval.start, "end": interval.end}))
                    .collect();
                Ok(StaffDayWrite {
                    day: date,
                    intervals: json!(intervals),
                })
            })
            .collect::<Result<Vec<_>, IngestError>>()?;
        Ok(Self {
            staff_id: payload.staff_id,
            days,
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
            CrmAction::EditCatalog,
            false,
        )
        .await?;
        let outcome = store::staff::write_staff_days(
            tx,
            tenant.community(),
            channel_id,
            self.staff_id,
            &self.days,
            event.id.as_bytes(),
        )
        .await
        .map_err(|e| db_error("write CRM staff days", e))?;
        match outcome {
            ScheduleWriteOutcome::Written => Ok(()),
            ScheduleWriteOutcome::UnknownStaff => Err(IngestError::Rejected(
                "invalid: staff_id is not a staff member of this space".into(),
            )),
            ScheduleWriteOutcome::ArchivedStaff => Err(IngestError::Rejected(
                "invalid: the staff member is archived".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staff_outcomes_map_to_conflicts_or_rejections() {
        for outcome in [
            StaffWriteOutcome::AlreadyExists,
            StaffWriteOutcome::Missing,
            StaffWriteOutcome::RevisionMismatch,
        ] {
            assert!(matches!(
                staff_outcome(outcome),
                Err(IngestError::CanvasConflict(_))
            ));
        }
        assert!(matches!(
            staff_outcome(StaffWriteOutcome::UnknownService(Uuid::nil())),
            Err(IngestError::Rejected(message)) if message.starts_with("invalid: ")
        ));
        assert!(staff_outcome(StaffWriteOutcome::Written).is_ok());
    }
}
