//! Fiscal receipts (kind 42120) and cash-register shifts (kind 42121).

use nostr::Event;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use buzz_core::tenant::TenantContext;
use buzz_crm::{tags, CashShiftPayload, CrmAction, FiscalReceiptPayload};
use buzz_db::crm::{self as store, ReceiptOutcome};

use super::db_error;
use super::space::{conflict, invalid, require_live_space, tag_slices};
use crate::handlers::ingest::IngestError;

fn rejected(message: impl std::fmt::Display) -> IngestError {
    IngestError::Rejected(format!("invalid: {message}"))
}

fn as_i64(value: u64, field: &str) -> Result<i64, IngestError> {
    i64::try_from(value).map_err(|_| rejected(format!("{field} is out of range")))
}

/// A kind 42120 event whose payload and tags passed pure validation.
pub(super) struct PreparedReceipt {
    sale_id: Uuid,
    payload: FiscalReceiptPayload,
}

impl PreparedReceipt {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let sale_id = tags::crm_id(tag_slices(event)).map_err(invalid)?;
        let payload = FiscalReceiptPayload::parse(&event.content).map_err(invalid)?;
        Ok(Self { sale_id, payload })
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
            CrmAction::RecordFiscal,
            false,
        )
        .await?;
        let fiscal = &self.payload.fiscal;
        let task_uuid =
            Uuid::parse_str(&self.payload.task_uuid).map_err(|_| rejected("task_uuid"))?;
        let receipt = store::ReceiptWrite {
            sale_id: self.sale_id,
            space_channel_id: channel_id,
            operation: self.payload.operation.as_str(),
            total_minor: self.payload.total_minor.minor(),
            task_uuid,
            document_number: as_i64(fiscal.document_number, "fiscal.document_number")?,
            document_sign: &fiscal.document_sign,
            receipt_number: as_i64(fiscal.receipt_number, "fiscal.receipt_number")?,
            shift_number: as_i64(fiscal.shift_number, "fiscal.shift_number")?,
            fn_number: &fiscal.fn_number,
            registration_number: fiscal.registration_number.as_deref(),
            document_datetime: &fiscal.document_datetime,
            event_id: event.id.as_bytes(),
            actor: &actor,
        };
        let outcome = store::fiscal::record_receipt(tx, tenant.community(), &receipt)
            .await
            .map_err(|e| db_error("record CRM fiscal receipt", e))?;
        receipt_outcome(outcome)
    }
}

fn receipt_outcome(outcome: ReceiptOutcome) -> Result<(), IngestError> {
    match outcome {
        ReceiptOutcome::Recorded => Ok(()),
        ReceiptOutcome::SaleMissing => Err(rejected("crm-id: not a sale of this CRM space")),
        ReceiptOutcome::AlreadyRecorded => {
            Err(conflict("this receipt is already recorded for the sale"))
        }
        ReceiptOutcome::TotalMismatch { expected } => Err(rejected(format!(
            "total_minor: the sale total is {expected} kopecks"
        ))),
        ReceiptOutcome::NotReturnable => Err(rejected(
            "operation: a return receipt needs a voided sale with a sale receipt",
        )),
        ReceiptOutcome::DocumentTaken => Err(conflict(
            "this fiscal document number is already recorded for the ФН",
        )),
    }
}

/// A kind 42121 event whose payload passed pure validation.
pub(super) struct PreparedShift {
    payload: CashShiftPayload,
}

impl PreparedShift {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let payload = CashShiftPayload::parse(&event.content).map_err(invalid)?;
        Ok(Self { payload })
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
            CrmAction::RecordFiscal,
            false,
        )
        .await?;
        let payload = &self.payload;
        let shift = store::ShiftWrite {
            space_channel_id: channel_id,
            action: payload.action.as_str(),
            fn_number: &payload.fn_number,
            shift_number: as_i64(payload.shift_number, "shift_number")?,
            document_number: as_i64(payload.document_number, "document_number")?,
            document_datetime: &payload.document_datetime,
            actor: &actor,
        };
        match store::fiscal::record_shift(tx, tenant.community(), &shift)
            .await
            .map_err(|e| db_error("record CRM cash shift", e))?
        {
            store::ShiftOutcome::Recorded => Ok(()),
            store::ShiftOutcome::AlreadyRecorded => Err(conflict(&format!(
                "shift {} is already recorded as {}",
                payload.shift_number,
                payload.action.as_str()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_outcomes_map_to_relay_prefixes() {
        assert!(receipt_outcome(ReceiptOutcome::Recorded).is_ok());
        for outcome in [
            ReceiptOutcome::AlreadyRecorded,
            ReceiptOutcome::DocumentTaken,
        ] {
            assert!(matches!(
                receipt_outcome(outcome),
                Err(IngestError::CanvasConflict(m)) if m.starts_with("conflict: ")
            ));
        }
        for outcome in [
            ReceiptOutcome::SaleMissing,
            ReceiptOutcome::NotReturnable,
            ReceiptOutcome::TotalMismatch { expected: 100 },
        ] {
            assert!(matches!(
                receipt_outcome(outcome),
                Err(IngestError::Rejected(m)) if m.starts_with("invalid: ")
            ));
        }
    }
}
