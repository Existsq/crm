//! Staff pay rates (kind 42160), payouts (42161) and payout cancellations
//! (42162). Only space owners and admins manage money paid to staff.

use chrono::NaiveDate;
use nostr::Event;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use buzz_core::tenant::TenantContext;
use buzz_crm::{tags, CrmAction, CrmError, PayRatesPayload, PayoutPayload, PayoutVoidPayload};
use buzz_db::crm::payroll::{
    self as store, PayRatesWrite, PayoutVoidOutcome, PayoutWrite, PayrollWriteOutcome,
};

use super::db_error;
use super::space::{conflict, invalid, require_live_space, tag_slices};
use crate::handlers::ingest::IngestError;

/// A kind 42160 event whose payload passed pure validation.
pub(super) struct PreparedPayRates(PayRatesPayload);

impl PreparedPayRates {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        PayRatesPayload::parse(&event.content)
            .map(Self)
            .map_err(invalid)
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
        let rates = &self.0;
        let percent_bp = i32::try_from(rates.percent_bp)
            .map_err(|_| invalid(CrmError::field("percent_bp", "is out of range")))?;
        let outcome = store::write_pay_rates(
            tx,
            tenant.community(),
            PayRatesWrite {
                space_channel_id: channel_id,
                staff_id: rates.staff_id,
                percent_bp,
                per_visit_minor: rates.per_visit_minor,
                shift_minor: rates.shift_minor,
                revision_event_id: event.id.as_bytes(),
            },
        )
        .await
        .map_err(|e| db_error("write CRM pay rates", e))?;
        write_outcome(outcome)
    }
}

/// A kind 42161 event whose payload and tags passed pure validation.
pub(super) struct PreparedPayout {
    payout_id: Uuid,
    day: NaiveDate,
    payload: PayoutPayload,
}

impl PreparedPayout {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let payout_id = tags::crm_id(tag_slices(event)).map_err(invalid)?;
        let payload = PayoutPayload::parse(&event.content).map_err(invalid)?;
        let day = NaiveDate::parse_from_str(&payload.date, "%Y-%m-%d")
            .map_err(|_| invalid(CrmError::field("date", "is not a YYYY-MM-DD date")))?;
        Ok(Self {
            payout_id,
            day,
            payload,
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
        let outcome = store::record_payout(
            tx,
            tenant.community(),
            PayoutWrite {
                payout_id: self.payout_id,
                space_channel_id: channel_id,
                staff_id: self.payload.staff_id,
                day: self.day,
                amount_minor: self.payload.amount_minor,
                note: self.payload.note.as_deref(),
                event_id: event.id.as_bytes(),
                actor: &actor,
            },
        )
        .await
        .map_err(|e| db_error("record CRM payout", e))?;
        write_outcome(outcome)
    }
}

fn write_outcome(outcome: PayrollWriteOutcome) -> Result<(), IngestError> {
    match outcome {
        PayrollWriteOutcome::Written => Ok(()),
        PayrollWriteOutcome::UnknownStaff => Err(IngestError::Rejected(
            "invalid: staff_id is not a staff member of this space".into(),
        )),
        PayrollWriteOutcome::AlreadyExists => Err(conflict("the payout is already recorded")),
    }
}

/// A kind 42162 event whose payload passed pure validation.
pub(super) struct PreparedPayoutVoid(PayoutVoidPayload);

impl PreparedPayoutVoid {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        PayoutVoidPayload::parse(&event.content)
            .map(Self)
            .map_err(invalid)
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
        let outcome = store::void_payout(
            tx,
            tenant.community(),
            channel_id,
            self.0.payout_id,
            event.id.as_bytes(),
        )
        .await
        .map_err(|e| db_error("void CRM payout", e))?;
        void_outcome(outcome)
    }
}

fn void_outcome(outcome: PayoutVoidOutcome) -> Result<(), IngestError> {
    match outcome {
        PayoutVoidOutcome::Voided => Ok(()),
        PayoutVoidOutcome::Missing => Err(IngestError::Rejected(
            "invalid: payout_id is not a payout of this space".into(),
        )),
        PayoutVoidOutcome::AlreadyVoided => Err(conflict("the payout is already cancelled")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_map_to_rejections_and_conflicts() {
        assert!(write_outcome(PayrollWriteOutcome::Written).is_ok());
        assert!(matches!(
            write_outcome(PayrollWriteOutcome::UnknownStaff),
            Err(IngestError::Rejected(message)) if message.starts_with("invalid: ")
        ));
        assert!(matches!(
            write_outcome(PayrollWriteOutcome::AlreadyExists),
            Err(IngestError::CanvasConflict(_))
        ));
        assert!(void_outcome(PayoutVoidOutcome::Voided).is_ok());
        assert!(matches!(
            void_outcome(PayoutVoidOutcome::Missing),
            Err(IngestError::Rejected(_))
        ));
        assert!(matches!(
            void_outcome(PayoutVoidOutcome::AlreadyVoided),
            Err(IngestError::CanvasConflict(_))
        ));
    }
}
