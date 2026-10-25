//! Product catalog (kind 42102).

use nostr::Event;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use buzz_core::tenant::TenantContext;
use buzz_crm::{tags, CrmAction, ProductPayload};
use buzz_db::crm::{self as store, ProductWriteOutcome};

use super::db_error;
use super::space::{conflict, invalid, require_live_space, tag_slices};
use crate::handlers::ingest::IngestError;

/// A kind 42102 event whose payload and tags passed pure validation.
pub(super) struct PreparedProduct {
    product_id: Uuid,
    payload: ProductPayload,
    expected_revision: Option<[u8; 32]>,
}

impl PreparedProduct {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let product_id = tags::crm_id(tag_slices(event)).map_err(invalid)?;
        let expected_revision = tags::expected_revision(tag_slices(event)).map_err(invalid)?;
        let payload = ProductPayload::parse(&event.content).map_err(invalid)?;
        Ok(Self {
            product_id,
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
        // Shared lock: catalog writes run concurrently with each other and
        // with document postings; uniqueness is the indexes' job.
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
        let write = store::ProductWrite {
            product_id: self.product_id,
            space_channel_id: channel_id,
            name: &payload.name,
            sku: payload.sku.as_deref(),
            barcodes: &payload.barcodes,
            unit: payload.unit.as_str(),
            price_minor: payload.price_minor.minor(),
            vat: payload.vat.as_str(),
            payment_object: payload.payment_object.as_str(),
            category: payload.category.as_deref(),
            min_stock_milli: payload.min_stock_milli.milli(),
            requires_marking: payload.requires_marking,
            duration_min: payload.duration_min.map(|m| m as i32),
            archived: payload.archived,
            revision_event_id: event.id.as_bytes(),
            actor: &actor,
        };
        let outcome = store::catalog::write_product(
            tx,
            tenant.community(),
            write,
            self.expected_revision.as_ref().map(|r| r.as_slice()),
        )
        .await
        .map_err(|e| db_error("write CRM product", e))?;
        product_outcome(outcome)
    }
}

fn product_outcome(outcome: ProductWriteOutcome) -> Result<(), IngestError> {
    match outcome {
        ProductWriteOutcome::Written => Ok(()),
        ProductWriteOutcome::AlreadyExists => Err(conflict(
            "product already exists; send expected-revision to update it",
        )),
        ProductWriteOutcome::Missing => Err(conflict("product does not exist")),
        ProductWriteOutcome::RevisionMismatch => {
            Err(conflict("product changed since it was loaded"))
        }
        ProductWriteOutcome::SkuTaken(Some(sku)) => Err(conflict(&format!(
            "SKU {sku:?} is already used by another product"
        ))),
        ProductWriteOutcome::SkuTaken(None) => {
            Err(conflict("SKU is already used by another product"))
        }
        ProductWriteOutcome::BarcodeTaken(Some(barcode)) => Err(conflict(&format!(
            "barcode {barcode} is already used by another product"
        ))),
        ProductWriteOutcome::BarcodeTaken(None) => {
            Err(conflict("a barcode is already used by another product"))
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
    fn duplicates_name_the_value_and_map_to_conflict() {
        assert_eq!(
            message(product_outcome(ProductWriteOutcome::SkuTaken(Some(
                "MLK-1".into()
            )))),
            "conflict: SKU \"MLK-1\" is already used by another product"
        );
        assert_eq!(
            message(product_outcome(ProductWriteOutcome::BarcodeTaken(Some(
                "4006381333931".into()
            )))),
            "conflict: barcode 4006381333931 is already used by another product"
        );
        for outcome in [
            ProductWriteOutcome::AlreadyExists,
            ProductWriteOutcome::Missing,
            ProductWriteOutcome::RevisionMismatch,
            ProductWriteOutcome::SkuTaken(None),
            ProductWriteOutcome::BarcodeTaken(None),
        ] {
            assert!(message(product_outcome(outcome)).starts_with("conflict: "));
        }
        assert!(product_outcome(ProductWriteOutcome::Written).is_ok());
    }
}
