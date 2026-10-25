//! Sale documents (kind 42104) and their reversal (kind 42105).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use nostr::Event;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use buzz_core::tenant::TenantContext;
use buzz_crm::{tags, CrmAction, CrmError, PartyType, SalePayload, SaleVoidPayload, Unit};
use buzz_db::crm::{self as store, AppointmentPayOutcome, SaleVoidOutcome};

use super::space::{conflict, invalid, require_live_space, tag_slices};
use super::{db_error, stock};
use crate::handlers::ingest::IngestError;

fn rejected(message: impl std::fmt::Display) -> IngestError {
    IngestError::Rejected(format!("invalid: {message}"))
}

/// Sales are posted once: an `expected-revision` tag means the client
/// mistook the sale for editable master data.
fn reject_revision(event: &Event, kind: &str) -> Result<(), IngestError> {
    match tags::expected_revision(tag_slices(event)).map_err(invalid)? {
        Some(_) => Err(rejected(format!(
            "{kind} is posted once and takes no expected-revision"
        ))),
        None => Ok(()),
    }
}

/// A kind 42104 event whose payload and tags passed pure validation.
pub(super) struct PreparedSale {
    sale_id: Uuid,
    payload: SalePayload,
}

impl PreparedSale {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let sale_id = tags::crm_id(tag_slices(event)).map_err(invalid)?;
        reject_revision(event, "a sale")?;
        let payload = SalePayload::parse(&event.content).map_err(invalid)?;
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
        let community = tenant.community();
        // Exclusive: sale numbers are assigned under the space row lock.
        require_live_space(
            tx,
            tenant,
            channel_id,
            &actor,
            CrmAction::PostDocuments,
            true,
        )
        .await?;
        if store::sales::sale_exists(tx, community, self.sale_id)
            .await
            .map_err(|e| db_error("read CRM sale", e))?
        {
            return Err(conflict("sale already exists"));
        }
        let payload = &self.payload;

        match store::sales::warehouse_for_sale(tx, community, channel_id, payload.warehouse_id)
            .await
            .map_err(|e| db_error("read CRM warehouse", e))?
        {
            None => return Err(rejected("warehouse_id: not a warehouse of this CRM space")),
            Some(true) => return Err(rejected("warehouse_id: the warehouse is archived")),
            Some(false) => {}
        }

        if let Some(customer_id) = payload.customer_id {
            let party = store::parties::party_for_sale(tx, community, channel_id, customer_id)
                .await
                .map_err(|e| db_error("read CRM party", e))?
                .ok_or_else(|| rejected("customer_id: not a party of this CRM space"))?;
            check_customer(&party)?;
        }

        let product_ids: Vec<Uuid> = payload.lines.iter().map(|line| line.product_id).collect();
        let products: HashMap<Uuid, store::sales::ProductForSale> =
            store::sales::products_for_sale(tx, community, channel_id, &product_ids)
                .await
                .map_err(|e| db_error("read CRM products", e))?
                .into_iter()
                .map(|product| (product.product_id, product))
                .collect();

        let mut lines = Vec::with_capacity(payload.lines.len());
        for line in &payload.lines {
            let product = products.get(&line.product_id).ok_or_else(|| {
                rejected(format!(
                    "lines: product {} is not in the catalog",
                    line.product_id
                ))
            })?;
            check_line_product(product, line.qty_milli)?;
            lines.push(store::SaleLineInsert {
                product_id: line.product_id,
                product_name: &product.name,
                unit: &product.unit,
                vat: &product.vat,
                qty_milli: line.qty_milli.milli(),
                price_minor: line.price_minor.minor(),
                discount_minor: line.discount_minor.minor(),
                amount_minor: line.amount().map_err(invalid)?.minor(),
            });
        }

        let parts = payload.payment_parts().map_err(invalid)?;
        let payments: Vec<(&str, i64)> = parts
            .iter()
            .map(|part| (part.method.as_str(), part.amount_minor.minor()))
            .collect();
        let sold_at = DateTime::<Utc>::from_timestamp(event.created_at.as_secs() as i64, 0)
            .ok_or_else(|| rejected("created_at is out of range"))?;
        let sale = store::SaleInsert {
            sale_id: self.sale_id,
            space_channel_id: channel_id,
            warehouse_id: payload.warehouse_id,
            customer_id: payload.customer_id,
            payment: payload.payment.as_str(),
            total_minor: payload.total().map_err(invalid)?.minor(),
            discount_minor: payload.discount_total().map_err(invalid)?.minor(),
            note: payload.note.as_deref(),
            sold_at,
            event_id: event.id.as_bytes(),
            actor: &actor,
            lines: &lines,
            payments: &payments,
            cash_tendered_minor: payload.cash_tendered_minor.map(|cash| cash.minor()),
        };
        store::sales::insert_sale(tx, community, &sale)
            .await
            .map_err(|e| db_error("insert CRM sale", e))?;
        if let Some(appointment_id) = payload.appointment_id {
            let outcome = store::appointments::pay_appointment(
                tx,
                community,
                channel_id,
                appointment_id,
                self.sale_id,
                event.id.as_bytes(),
            )
            .await
            .map_err(|e| db_error("pay CRM appointment", e))?;
            pay_outcome(outcome)?;
        }
        let sold: Vec<stock::SoldLine> = payload
            .lines
            .iter()
            .map(|line| stock::SoldLine {
                product_id: line.product_id,
                qty: line.qty_milli,
            })
            .collect();
        stock::post_sale(
            tx,
            community,
            channel_id,
            self.sale_id,
            payload.warehouse_id,
            &sold,
            event,
        )
        .await
    }
}

fn check_customer(party: &store::parties::PartyForSale) -> Result<(), IngestError> {
    if party.archived {
        return Err(rejected("customer_id: the customer is archived"));
    }
    let party_type: PartyType = serde_json::from_value(party.party_type.clone().into())
        .map_err(|e| IngestError::Internal(format!("error: stored party type: {e}")))?;
    if !party_type.buys() {
        return Err(rejected(
            "customer_id: the party is a supplier, not a customer",
        ));
    }
    Ok(())
}

fn check_line_product(
    product: &store::sales::ProductForSale,
    qty: buzz_crm::Qty,
) -> Result<(), IngestError> {
    if product.archived {
        return Err(rejected(format!(
            "lines: product {:?} is archived",
            product.name
        )));
    }
    let unit: Unit = serde_json::from_value(product.unit.clone().into())
        .map_err(|e| IngestError::Internal(format!("error: stored product unit: {e}")))?;
    unit.check_qty(qty, "lines.qty_milli")
        .map_err(|error: CrmError| rejected(format!("{error} ({})", product.name)))
}

/// A kind 42105 event whose payload and tags passed pure validation.
pub(super) struct PreparedSaleVoid {
    sale_id: Uuid,
    payload: SaleVoidPayload,
}

impl PreparedSaleVoid {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let sale_id = tags::crm_id(tag_slices(event)).map_err(invalid)?;
        reject_revision(event, "a sale void")?;
        let payload = SaleVoidPayload::parse(&event.content).map_err(invalid)?;
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
            CrmAction::VoidDocuments,
            // Exclusive: the returned goods change stock levels.
            true,
        )
        .await?;
        let outcome = store::sales::void_sale(
            tx,
            tenant.community(),
            channel_id,
            self.sale_id,
            event.id.as_bytes(),
            &self.payload.reason,
            &actor,
        )
        .await
        .map_err(|e| db_error("void CRM sale", e))?;
        void_outcome(outcome)?;
        store::appointments::unpay_appointment(
            tx,
            tenant.community(),
            self.sale_id,
            event.id.as_bytes(),
        )
        .await
        .map_err(|e| db_error("unpay CRM appointment", e))?;
        // Returned goods go back on the shelf whatever the negative-stock rule.
        stock::revert_moves(
            tx,
            tenant.community(),
            channel_id,
            self.sale_id,
            true,
            event,
        )
        .await
    }
}

fn pay_outcome(outcome: AppointmentPayOutcome) -> Result<(), IngestError> {
    match outcome {
        AppointmentPayOutcome::Paid => Ok(()),
        AppointmentPayOutcome::Missing => Err(rejected(
            "appointment_id: not an appointment of this CRM space",
        )),
        AppointmentPayOutcome::AlreadyPaid => Err(conflict("the visit is already paid")),
        AppointmentPayOutcome::NotPayable => Err(rejected(
            "appointment_id: a cancelled or missed visit cannot be paid",
        )),
    }
}

fn void_outcome(outcome: SaleVoidOutcome) -> Result<(), IngestError> {
    match outcome {
        SaleVoidOutcome::Voided => Ok(()),
        SaleVoidOutcome::Missing => Err(conflict("sale does not exist")),
        SaleVoidOutcome::AlreadyVoided => Err(conflict("sale is already voided")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_crm::Qty;

    fn product(unit: &str, archived: bool) -> store::sales::ProductForSale {
        store::sales::ProductForSale {
            product_id: Uuid::nil(),
            name: "Кефир".into(),
            unit: unit.into(),
            vat: "vat10".into(),
            archived,
        }
    }

    fn rejection(result: Result<(), IngestError>) -> String {
        match result {
            Err(IngestError::Rejected(message)) => message,
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    #[test]
    fn pieces_sell_in_whole_units_and_archived_products_not_at_all() {
        let half = Qty::from_milli(500).unwrap();
        let one = Qty::from_milli(1000).unwrap();
        assert!(check_line_product(&product("piece", false), one).is_ok());
        assert!(check_line_product(&product("kilogram", false), half).is_ok());
        assert!(
            rejection(check_line_product(&product("piece", false), half))
                .starts_with("invalid: lines.qty_milli")
        );
        assert!(rejection(check_line_product(&product("piece", true), one)).contains("archived"));
    }

    #[test]
    fn only_live_buyers_can_be_customers() {
        let party = |party_type: &str, archived| store::parties::PartyForSale {
            party_type: party_type.into(),
            archived,
        };
        assert!(check_customer(&party("customer", false)).is_ok());
        assert!(check_customer(&party("both", false)).is_ok());
        assert!(rejection(check_customer(&party("supplier", false))).contains("supplier"));
        assert!(rejection(check_customer(&party("customer", true))).contains("archived"));
    }

    #[test]
    fn visit_payments_map_to_relay_prefixes() {
        assert!(pay_outcome(AppointmentPayOutcome::Paid).is_ok());
        assert!(matches!(
            pay_outcome(AppointmentPayOutcome::AlreadyPaid),
            Err(IngestError::CanvasConflict(message)) if message.starts_with("conflict: ")
        ));
        for outcome in [
            AppointmentPayOutcome::Missing,
            AppointmentPayOutcome::NotPayable,
        ] {
            assert!(rejection(pay_outcome(outcome)).starts_with("invalid: appointment_id"));
        }
    }

    #[test]
    fn void_outcomes_map_to_conflict() {
        assert!(void_outcome(SaleVoidOutcome::Voided).is_ok());
        for outcome in [SaleVoidOutcome::Missing, SaleVoidOutcome::AlreadyVoided] {
            assert!(matches!(
                void_outcome(outcome),
                Err(IngestError::CanvasConflict(message)) if message.starts_with("conflict: ")
            ));
        }
    }
}
