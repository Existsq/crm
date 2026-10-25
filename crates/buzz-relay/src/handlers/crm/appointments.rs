//! Appointments (kind 42133): a client booked with one staff member.

use chrono::NaiveDate;
use nostr::Event;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use buzz_core::tenant::TenantContext;
use buzz_crm::{format_clock, tags, AppointmentPayload, CrmAction, CrmError};
use buzz_db::crm::{self as store, AppointmentServiceWrite, AppointmentWriteOutcome};

use super::db_error;
use super::space::{conflict, invalid, require_live_space, tag_slices};
use crate::handlers::ingest::IngestError;

/// A kind 42133 event whose payload and tags passed pure validation.
pub(super) struct PreparedAppointment {
    appointment_id: Uuid,
    day: NaiveDate,
    payload: AppointmentPayload,
    expected_revision: Option<[u8; 32]>,
}

impl PreparedAppointment {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let appointment_id = tags::crm_id(tag_slices(event)).map_err(invalid)?;
        let expected_revision = tags::expected_revision(tag_slices(event)).map_err(invalid)?;
        let payload = AppointmentPayload::parse(&event.content).map_err(invalid)?;
        let day = NaiveDate::parse_from_str(&payload.date, "%Y-%m-%d")
            .map_err(|_| invalid(CrmError::field("date", "is not a YYYY-MM-DD date")))?;
        Ok(Self {
            appointment_id,
            day,
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
            CrmAction::PostDocuments,
            false,
        )
        .await?;
        project(
            tx,
            tenant,
            event,
            channel_id,
            self.appointment_id,
            self.day,
            &self.payload,
            self.expected_revision.as_ref().map(|r| r.as_slice()),
        )
        .await
    }
}

/// Write the appointment row of a validated payload (shared with the
/// relay-signed online bookings of kind 42134, which skip the role check).
#[allow(clippy::too_many_arguments)]
pub(super) async fn project(
    tx: &mut Transaction<'static, Postgres>,
    tenant: &TenantContext,
    event: &Event,
    channel_id: Uuid,
    appointment_id: Uuid,
    day: NaiveDate,
    payload: &AppointmentPayload,
    expected_revision: Option<&[u8]>,
) -> Result<(), IngestError> {
    let actor = event.pubkey.to_bytes();
    let services: Vec<AppointmentServiceWrite> = payload
        .services
        .iter()
        .map(|service| AppointmentServiceWrite {
            product_id: service.product_id,
            // Bounded to 720 by `AppointmentPayload::parse`.
            duration_min: service.duration_min as i32,
            price_minor: service.price_minor.minor(),
        })
        .collect();
    let write = store::AppointmentWrite {
        appointment_id,
        space_channel_id: channel_id,
        staff_id: payload.staff_id,
        day,
        start_min: i32::from(payload.start),
        end_min: i32::from(payload.end()),
        services: &services,
        customer_id: payload.customer_id,
        client_name: payload.client_name.as_deref(),
        client_phone: payload.client_phone.as_deref(),
        status: payload.status.as_str(),
        holds_time: payload.status.holds_time(),
        note: payload.note.as_deref(),
        source: payload.source.as_str(),
        outside_hours: payload.outside_hours,
        revision_event_id: event.id.as_bytes(),
        actor: &actor,
    };
    let outcome =
        store::appointments::write_appointment(tx, tenant.community(), write, expected_revision)
            .await
            .map_err(|e| db_error("write CRM appointment", e))?;
    appointment_outcome(outcome)
}

fn rejected(message: String) -> IngestError {
    IngestError::Rejected(format!("invalid: {message}"))
}

fn clock(minutes: i32) -> String {
    u16::try_from(minutes).map_or_else(|_| minutes.to_string(), format_clock)
}

fn appointment_outcome(outcome: AppointmentWriteOutcome) -> Result<(), IngestError> {
    use AppointmentWriteOutcome as Out;
    match outcome {
        Out::Written => Ok(()),
        Out::AlreadyExists => Err(conflict(
            "appointment already exists; send expected-revision to update it",
        )),
        Out::Missing => Err(conflict("appointment does not exist")),
        Out::RevisionMismatch => Err(conflict("appointment changed since it was loaded")),
        Out::UnknownStaff => Err(rejected(
            "staff_id is not a staff member of this space".into(),
        )),
        Out::ArchivedStaff => Err(rejected("the staff member is archived".into())),
        Out::UnknownService(id) => Err(rejected(format!(
            "services: {id} is not an active service of this space"
        ))),
        Out::ServiceNotOffered(id) => Err(rejected(format!(
            "services: the staff member does not perform {id}"
        ))),
        Out::UnknownCustomer => Err(rejected(
            "customer_id is not a customer of this space".into(),
        )),
        Out::OutsideHours => Err(rejected(
            "the visit is outside the staff member's working hours".into(),
        )),
        // A slot taken a moment ago: the client reloads and picks another.
        Out::Overlap { start, end } => Err(conflict(&format!(
            "the staff member is booked {}–{}",
            clock(start),
            clock(end)
        ))),
        Out::Paid => Err(rejected(
            "the visit is paid; its staff, time and services cannot change".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlaps_are_conflicts_and_rule_breaks_are_rejections() {
        let message = |outcome| match appointment_outcome(outcome) {
            Err(IngestError::CanvasConflict(m)) => format!("409 {m}"),
            Err(IngestError::Rejected(m)) => format!("400 {m}"),
            Err(other) => format!("{other:?}"),
            Ok(()) => "ok".into(),
        };
        assert_eq!(
            message(AppointmentWriteOutcome::Overlap {
                start: 600,
                end: 660
            }),
            "409 conflict: the staff member is booked 10:00–11:00"
        );
        assert!(message(AppointmentWriteOutcome::OutsideHours).starts_with("400 invalid: "));
        assert!(message(AppointmentWriteOutcome::Paid).starts_with("400 invalid: "));
        assert!(message(AppointmentWriteOutcome::RevisionMismatch).starts_with("409 conflict: "));
        assert_eq!(message(AppointmentWriteOutcome::Written), "ok");
    }
}
