//! Stock documents (kinds 42106, 42107), service tech cards (kind 42108) and
//! the stock movements of sales (kinds 42104, 42105).
//!
//! Every writer here runs under the exclusive space row lock, reads the
//! affected levels `FOR UPDATE`, plans the movements with `buzz_crm::stock`
//! and writes moves and levels in the same transaction as the event.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use nostr::Event;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use buzz_core::tenant::TenantContext;
use buzz_core::CommunityId;
use buzz_crm::stock::{is_stocked, scale_qty};
use buzz_crm::{
    tags, CrmAction, CrmError, Money, PartyType, PaymentObject, Qty, SaleVoidPayload, StockDelta,
    StockDocPayload, StockDocType, StockLevel, TechCardPayload, Unit,
};
use buzz_db::crm::stock::{self as store, ProductForStock, StockMoveWrite};
use buzz_db::crm::{StockDocVoidOutcome, TechCardWriteOutcome};

use super::db_error;
use super::space::{conflict, invalid, require_live_space, tag_slices};
use crate::handlers::ingest::IngestError;

fn rejected(message: impl std::fmt::Display) -> IngestError {
    IngestError::Rejected(format!("invalid: {message}"))
}

fn stored<T: serde::de::DeserializeOwned>(raw: &str, what: &str) -> Result<T, IngestError> {
    serde_json::from_value(raw.into())
        .map_err(|e| IngestError::Internal(format!("error: stored {what}: {e}")))
}

/// `1500` thousandths → `"1.5"`, for rejection messages.
fn units(milli: i64) -> String {
    let sign = if milli < 0 { "-" } else { "" };
    let abs = milli.unsigned_abs();
    let (whole, frac) = (abs / 1000, abs % 1000);
    if frac == 0 {
        format!("{sign}{whole}")
    } else {
        format!(
            "{sign}{whole}.{}",
            format!("{frac:03}").trim_end_matches('0')
        )
    }
}

fn posted_at(event: &Event) -> Result<DateTime<Utc>, IngestError> {
    DateTime::<Utc>::from_timestamp(event.created_at.as_secs() as i64, 0)
        .ok_or_else(|| rejected("created_at is out of range"))
}

/// Stock levels of one write, as locked and as changed so far.
struct Ledger {
    levels: HashMap<(Uuid, Uuid), StockLevel>,
    moves: Vec<StockMoveWrite>,
}

impl Ledger {
    async fn lock(
        tx: &mut Transaction<'static, Postgres>,
        community: CommunityId,
        channel_id: Uuid,
        pairs: &[(Uuid, Uuid)],
    ) -> Result<Self, IngestError> {
        let rows = store::lock_levels(tx, community, channel_id, pairs)
            .await
            .map_err(|e| db_error("lock CRM stock", e))?;
        let levels = rows
            .into_iter()
            .map(|(key, (qty, value))| {
                StockLevel::new(qty, value)
                    .map(|level| (key, level))
                    .map_err(|e| IngestError::Internal(format!("error: stored stock level: {e}")))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            levels,
            moves: Vec::new(),
        })
    }

    fn level(&self, warehouse: Uuid, product: Uuid) -> StockLevel {
        self.levels
            .get(&(warehouse, product))
            .copied()
            .unwrap_or_default()
    }

    /// Record `delta` (no-op when zero) and return it.
    fn push(
        &mut self,
        warehouse: Uuid,
        product: Uuid,
        source: &'static str,
        reversal: bool,
        delta: StockDelta,
    ) -> Result<StockDelta, IngestError> {
        if delta.is_zero() {
            return Ok(delta);
        }
        let after = self
            .level(warehouse, product)
            .apply(delta)
            .map_err(|_| rejected("stock quantity out of range"))?;
        self.levels.insert((warehouse, product), after);
        self.moves.push(StockMoveWrite {
            warehouse_id: warehouse,
            product_id: product,
            source,
            reversal,
            qty_delta_milli: delta.qty.milli(),
            value_delta_minor: delta.value.minor(),
            qty_after_milli: after.qty.milli(),
            value_after_minor: after.value.minor(),
        });
        Ok(delta)
    }

    async fn write(
        self,
        tx: &mut Transaction<'static, Postgres>,
        community: CommunityId,
        channel_id: Uuid,
        doc_id: Uuid,
        event: &Event,
    ) -> Result<(), IngestError> {
        store::write_moves(
            tx,
            community,
            channel_id,
            doc_id,
            posted_at(event)?,
            event.id.as_bytes(),
            &self.moves,
        )
        .await
        .map_err(|e| db_error("write CRM stock moves", e))
    }
}

/// The forward moves a stock document reverts on void.
const DOC_SOURCES: &[&str] = &["receipt", "write_off", "inventory", "transfer"];
/// The forward moves a sale reverts on void.
pub(super) const SALE_SOURCES: &[&str] = &["sale", "consumable"];

/// A shortfall issuing `requested` of `product` with `on_hand` left.
fn short_of(product: &str, on_hand: Qty, requested: Qty, error: CrmError) -> IngestError {
    match error {
        CrmError::Field { .. } => rejected(format!(
            "lines: not enough stock of {product:?}: {} on hand, {} requested",
            units(on_hand.milli()),
            units(requested.milli())
        )),
        other => rejected(other),
    }
}

async fn products(
    tx: &mut Transaction<'static, Postgres>,
    community: CommunityId,
    channel_id: Uuid,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, ProductForStock>, IngestError> {
    Ok(store::products_for_stock(tx, community, channel_id, ids)
        .await
        .map_err(|e| db_error("read CRM products", e))?
        .into_iter()
        .map(|product| (product.product_id, product))
        .collect())
}

async fn require_warehouse(
    tx: &mut Transaction<'static, Postgres>,
    community: CommunityId,
    channel_id: Uuid,
    warehouse_id: Uuid,
    field: &str,
) -> Result<(), IngestError> {
    match store::warehouse_for_stock(tx, community, channel_id, warehouse_id)
        .await
        .map_err(|e| db_error("read CRM warehouse", e))?
    {
        None => Err(rejected(format!(
            "{field}: not a warehouse of this CRM space"
        ))),
        Some(true) => Err(rejected(format!("{field}: the warehouse is archived"))),
        Some(false) => Ok(()),
    }
}

/// A kind 42106 event whose payload and tags passed pure validation.
pub(super) struct PreparedStockDoc {
    doc_id: Uuid,
    payload: StockDocPayload,
}

impl PreparedStockDoc {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let doc_id = tags::crm_id(tag_slices(event)).map_err(invalid)?;
        if tags::expected_revision(tag_slices(event))
            .map_err(invalid)?
            .is_some()
        {
            return Err(rejected(
                "a stock document is posted once and takes no expected-revision",
            ));
        }
        let payload = StockDocPayload::parse(&event.content).map_err(invalid)?;
        Ok(Self { doc_id, payload })
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
        // Exclusive: numbering and stock levels serialize per space.
        require_live_space(
            tx,
            tenant,
            channel_id,
            &actor,
            CrmAction::PostDocuments,
            true,
        )
        .await?;
        if store::stock_doc_exists(tx, community, self.doc_id)
            .await
            .map_err(|e| db_error("read CRM stock document", e))?
        {
            return Err(conflict("stock document already exists"));
        }
        let payload = &self.payload;
        let doc_type = payload.doc_type;
        require_warehouse(
            tx,
            community,
            channel_id,
            payload.warehouse_id,
            "warehouse_id",
        )
        .await?;
        if let Some(to) = payload.to_warehouse_id {
            require_warehouse(tx, community, channel_id, to, "to_warehouse_id").await?;
        }
        if let Some(supplier_id) = payload.supplier_id {
            let party =
                buzz_db::crm::parties::party_for_sale(tx, community, channel_id, supplier_id)
                    .await
                    .map_err(|e| db_error("read CRM party", e))?
                    .ok_or_else(|| rejected("supplier_id: not a party of this CRM space"))?;
            if party.archived {
                return Err(rejected("supplier_id: the supplier is archived"));
            }
            if !stored::<PartyType>(&party.party_type, "party type")?.supplies() {
                return Err(rejected(
                    "supplier_id: the party is a customer, not a supplier",
                ));
            }
        }

        let ids: Vec<Uuid> = payload.lines.iter().map(|line| line.product_id).collect();
        let catalog = products(tx, community, channel_id, &ids).await?;
        for line in &payload.lines {
            let product = catalog.get(&line.product_id).ok_or_else(|| {
                rejected(format!(
                    "lines: product {} is not in the catalog",
                    line.product_id
                ))
            })?;
            check_stock_product(product, line.qty_milli, doc_type == StockDocType::Receipt)?;
        }

        let allow_negative = store::allows_negative_stock(tx, community, channel_id)
            .await
            .map_err(|e| db_error("read CRM settings", e))?;
        let mut pairs: Vec<(Uuid, Uuid)> =
            ids.iter().map(|id| (payload.warehouse_id, *id)).collect();
        if let Some(to) = payload.to_warehouse_id {
            pairs.extend(ids.iter().map(|id| (to, *id)));
        }
        let mut ledger = Ledger::lock(tx, community, channel_id, &pairs).await?;

        let source = doc_type.as_str();
        let wh = payload.warehouse_id;
        let mut lines = Vec::with_capacity(payload.lines.len());
        let mut total = Money::ZERO;
        for line in &payload.lines {
            let product = &catalog[&line.product_id];
            let level = ledger.level(wh, line.product_id);
            let (amount, book_qty) = match doc_type {
                StockDocType::Receipt => {
                    let cost = line.cost_minor.unwrap_or(Money::ZERO);
                    let value = buzz_crm::line_amount(cost, line.qty_milli).map_err(invalid)?;
                    let delta = level.receive(line.qty_milli, value).map_err(invalid)?;
                    ledger.push(wh, line.product_id, source, false, delta)?;
                    (value, None)
                }
                StockDocType::WriteOff => {
                    let delta = level
                        .issue(line.qty_milli, allow_negative)
                        .map_err(|e| short_of(&product.name, level.qty, line.qty_milli, e))?;
                    ledger.push(wh, line.product_id, source, false, delta)?;
                    (
                        Money::from_minor(-delta.value.minor()).map_err(invalid)?,
                        None,
                    )
                }
                StockDocType::Inventory => {
                    let delta = level
                        .count(line.qty_milli, line.cost_minor)
                        .map_err(invalid)?;
                    ledger.push(wh, line.product_id, source, false, delta)?;
                    let moved = Money::from_minor(delta.value.minor().abs()).map_err(invalid)?;
                    (moved, Some(level.qty.milli()))
                }
                StockDocType::Transfer => {
                    let to = payload
                        .to_warehouse_id
                        .ok_or_else(|| IngestError::Internal("error: validated transfer".into()))?;
                    let out = level
                        .issue(line.qty_milli, allow_negative)
                        .map_err(|e| short_of(&product.name, level.qty, line.qty_milli, e))?;
                    ledger.push(wh, line.product_id, source, false, out)?;
                    let value = Money::from_minor(-out.value.minor()).map_err(invalid)?;
                    let into = ledger
                        .level(to, line.product_id)
                        .receive(line.qty_milli, value)
                        .map_err(invalid)?;
                    ledger.push(to, line.product_id, source, false, into)?;
                    (value, None)
                }
            };
            total = total
                .checked_add(amount)
                .map_err(|_| rejected("lines: document total out of range"))?;
            lines.push(store::StockDocLineInsert {
                product_id: line.product_id,
                product_name: &product.name,
                unit: &product.unit,
                qty_milli: line.qty_milli.milli(),
                book_qty_milli: book_qty,
                cost_minor: line.cost_minor.map(Money::minor),
                amount_minor: amount.minor(),
            });
        }

        let doc = store::StockDocInsert {
            doc_id: self.doc_id,
            space_channel_id: channel_id,
            doc_type: doc_type.as_str(),
            warehouse_id: wh,
            to_warehouse_id: payload.to_warehouse_id,
            supplier_id: payload.supplier_id,
            reference: payload.reference.as_deref(),
            note: payload.note.as_deref(),
            total_minor: total.minor(),
            posted_at: posted_at(event)?,
            event_id: event.id.as_bytes(),
            actor: &actor,
            lines: &lines,
        };
        store::insert_stock_doc(tx, community, &doc)
            .await
            .map_err(|e| db_error("insert CRM stock document", e))?;
        ledger
            .write(tx, community, channel_id, self.doc_id, event)
            .await
    }
}

/// A product named on a stock line must be stocked goods, counted in its
/// unit; archived goods can still be written off, counted or moved, but not
/// received.
fn check_stock_product(
    product: &ProductForStock,
    qty: Qty,
    receiving: bool,
) -> Result<(), IngestError> {
    if !is_stocked(stored::<PaymentObject>(
        &product.payment_object,
        "payment object",
    )?) {
        return Err(rejected(format!(
            "lines: {:?} is a service, not stocked goods",
            product.name
        )));
    }
    if receiving && product.archived {
        return Err(rejected(format!(
            "lines: product {:?} is archived",
            product.name
        )));
    }
    stored::<Unit>(&product.unit, "product unit")?
        .check_qty(qty, "lines.qty_milli")
        .map_err(|error| rejected(format!("{error} ({})", product.name)))
}

/// A kind 42107 event whose payload and tags passed pure validation.
pub(super) struct PreparedStockDocVoid {
    doc_id: Uuid,
    payload: SaleVoidPayload,
}

impl PreparedStockDocVoid {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        let doc_id = tags::crm_id(tag_slices(event)).map_err(invalid)?;
        if tags::expected_revision(tag_slices(event))
            .map_err(invalid)?
            .is_some()
        {
            return Err(rejected(
                "a stock document void is posted once and takes no expected-revision",
            ));
        }
        let payload = SaleVoidPayload::parse(&event.content).map_err(invalid)?;
        Ok(Self { doc_id, payload })
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
        require_live_space(
            tx,
            tenant,
            channel_id,
            &actor,
            CrmAction::VoidDocuments,
            true,
        )
        .await?;
        let outcome = store::void_stock_doc(
            tx,
            community,
            channel_id,
            self.doc_id,
            posted_at(event)?,
            event.id.as_bytes(),
            &self.payload.reason,
            &actor,
        )
        .await
        .map_err(|e| db_error("void CRM stock document", e))?;
        match outcome {
            StockDocVoidOutcome::Voided => {}
            StockDocVoidOutcome::Missing => return Err(conflict("stock document does not exist")),
            StockDocVoidOutcome::AlreadyVoided => {
                return Err(conflict("stock document is already voided"))
            }
        }
        let allow_negative = store::allows_negative_stock(tx, community, channel_id)
            .await
            .map_err(|e| db_error("read CRM settings", e))?;
        revert_moves(
            tx,
            community,
            channel_id,
            self.doc_id,
            DOC_SOURCES,
            allow_negative,
            event,
        )
        .await
    }
}

/// Append the reversal of every forward move of `doc_id` (a stock document
/// or a sale, told apart by `sources`). `allow_negative` decides whether taking back received goods
/// may leave a level below zero.
pub(super) async fn revert_moves(
    tx: &mut Transaction<'static, Postgres>,
    community: CommunityId,
    channel_id: Uuid,
    doc_id: Uuid,
    sources: &[&str],
    allow_negative: bool,
    event: &Event,
) -> Result<(), IngestError> {
    let moves = store::moves_of_doc(tx, community, channel_id, doc_id, sources)
        .await
        .map_err(|e| db_error("read CRM stock moves", e))?;
    if moves.is_empty() {
        return Ok(());
    }
    let pairs: Vec<(Uuid, Uuid)> = moves
        .iter()
        .map(|m| (m.warehouse_id, m.product_id))
        .collect();
    let catalog = products(
        tx,
        community,
        channel_id,
        &pairs.iter().map(|(_, p)| *p).collect::<Vec<_>>(),
    )
    .await?;
    let mut ledger = Ledger::lock(tx, community, channel_id, &pairs).await?;
    // Newest first, so a transfer leaves the destination before it returns
    // to the source.
    for m in moves.iter().rev() {
        let original = StockDelta {
            qty: Qty::from_milli(m.qty_delta_milli).map_err(invalid)?,
            value: Money::from_minor(m.value_delta_minor).map_err(invalid)?,
        };
        let name = catalog
            .get(&m.product_id)
            .map_or_else(|| m.product_id.to_string(), |p| p.name.clone());
        let delta = ledger
            .level(m.warehouse_id, m.product_id)
            .revert(original, allow_negative)
            .map_err(|e| match e {
                CrmError::Field { .. } => rejected(format!(
                    "not enough stock of {name:?} to void the document: the goods were already used"
                )),
                other => rejected(other),
            })?;
        let source = source_name(&m.source)?;
        ledger.push(m.warehouse_id, m.product_id, source, true, delta)?;
    }
    ledger.write(tx, community, channel_id, doc_id, event).await
}

fn source_name(raw: &str) -> Result<&'static str, IngestError> {
    const SOURCES: [&str; 6] = [
        "receipt",
        "write_off",
        "inventory",
        "transfer",
        "sale",
        "consumable",
    ];
    SOURCES
        .into_iter()
        .find(|source| *source == raw)
        .ok_or_else(|| IngestError::Internal(format!("error: stored stock source {raw:?}")))
}

/// One posted sale line, as the stock ledger needs it.
pub(super) struct SoldLine {
    pub(super) product_id: Uuid,
    pub(super) qty: Qty,
}

/// Take the goods of a sale off `warehouse_id`, plus the consumables of its
/// service lines. Goods respect the space's negative-stock rule; consumables
/// never block a paid visit and may go below zero.
pub(super) async fn post_sale(
    tx: &mut Transaction<'static, Postgres>,
    community: CommunityId,
    channel_id: Uuid,
    sale_id: Uuid,
    warehouse_id: Uuid,
    lines: &[SoldLine],
    event: &Event,
) -> Result<(), IngestError> {
    let ids: Vec<Uuid> = lines.iter().map(|line| line.product_id).collect();
    let sold = products(tx, community, channel_id, &ids).await?;
    let mut goods = Vec::new();
    let mut services = Vec::new();
    for line in lines {
        let Some(product) = sold.get(&line.product_id) else {
            continue;
        };
        let object: PaymentObject = stored(&product.payment_object, "payment object")?;
        if is_stocked(object) {
            goods.push(line);
        } else if matches!(object, PaymentObject::Service | PaymentObject::Job) {
            services.push(line);
        }
    }
    let service_ids: Vec<Uuid> = services.iter().map(|line| line.product_id).collect();
    let consumables = store::consumables_for_services(tx, community, &service_ids)
        .await
        .map_err(|e| db_error("read CRM tech cards", e))?;
    if goods.is_empty() && consumables.is_empty() {
        return Ok(());
    }
    let service_qty: HashMap<Uuid, Qty> = services
        .iter()
        .map(|line| (line.product_id, line.qty))
        .collect();
    let mut used: Vec<(Uuid, Qty)> = Vec::new();
    for item in &consumables {
        let per_unit = Qty::from_milli(item.qty_milli).map_err(invalid)?;
        let units = service_qty
            .get(&item.service_id)
            .copied()
            .unwrap_or(Qty::ZERO);
        let qty = scale_qty(per_unit, units).map_err(invalid)?;
        if qty.milli() > 0 {
            used.push((item.product_id, qty));
        }
    }

    let mut pairs: Vec<(Uuid, Uuid)> = goods
        .iter()
        .map(|line| (warehouse_id, line.product_id))
        .collect();
    pairs.extend(used.iter().map(|(product, _)| (warehouse_id, *product)));
    let mut ledger = Ledger::lock(tx, community, channel_id, &pairs).await?;
    let allow_negative = store::allows_negative_stock(tx, community, channel_id)
        .await
        .map_err(|e| db_error("read CRM settings", e))?;
    for line in goods {
        let name = &sold[&line.product_id].name;
        let level = ledger.level(warehouse_id, line.product_id);
        let delta = level
            .issue(line.qty, allow_negative)
            .map_err(|e| short_of(name, level.qty, line.qty, e))?;
        ledger.push(warehouse_id, line.product_id, "sale", false, delta)?;
    }
    for (product, qty) in used {
        let delta = ledger
            .level(warehouse_id, product)
            .issue(qty, true)
            .map_err(invalid)?;
        ledger.push(warehouse_id, product, "consumable", false, delta)?;
    }
    ledger
        .write(tx, community, channel_id, sale_id, event)
        .await
}

/// A kind 42108 event whose payload and tags passed pure validation.
pub(super) struct PreparedTechCard {
    service_id: Uuid,
    payload: TechCardPayload,
    expected_revision: Option<[u8; 32]>,
}

impl PreparedTechCard {
    pub(super) fn parse(event: &Event) -> Result<Self, IngestError> {
        Ok(Self {
            service_id: tags::crm_id(tag_slices(event)).map_err(invalid)?,
            expected_revision: tags::expected_revision(tag_slices(event)).map_err(invalid)?,
            payload: TechCardPayload::parse(&event.content).map_err(invalid)?,
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
        let community = tenant.community();
        require_live_space(
            tx,
            tenant,
            channel_id,
            &actor,
            CrmAction::EditCatalog,
            false,
        )
        .await?;
        let mut ids: Vec<Uuid> = self
            .payload
            .items
            .iter()
            .map(|item| item.product_id)
            .collect();
        ids.push(self.service_id);
        let catalog = products(tx, community, channel_id, &ids).await?;
        let service = catalog
            .get(&self.service_id)
            .ok_or_else(|| rejected("crm-id: not a product of this CRM space"))?;
        let object: PaymentObject = stored(&service.payment_object, "payment object")?;
        if !matches!(object, PaymentObject::Service | PaymentObject::Job) {
            return Err(rejected(format!(
                "crm-id: {:?} is not a service; tech cards belong to services",
                service.name
            )));
        }
        let mut items = Vec::with_capacity(self.payload.items.len());
        for item in &self.payload.items {
            let product = catalog.get(&item.product_id).ok_or_else(|| {
                rejected(format!(
                    "items: product {} is not in the catalog",
                    item.product_id
                ))
            })?;
            check_stock_product(product, item.qty_milli, true).map_err(|e| match e {
                IngestError::Rejected(message) => {
                    IngestError::Rejected(message.replacen("lines", "items", 1))
                }
                other => other,
            })?;
            items.push((item.product_id, item.qty_milli.milli()));
        }
        let outcome = store::write_tech_card(
            tx,
            community,
            channel_id,
            self.service_id,
            &items,
            self.expected_revision.as_ref().map(|r| r.as_slice()),
            event.id.as_bytes(),
            &actor,
        )
        .await
        .map_err(|e| db_error("write CRM tech card", e))?;
        tech_card_outcome(outcome)
    }
}

fn tech_card_outcome(outcome: TechCardWriteOutcome) -> Result<(), IngestError> {
    match outcome {
        TechCardWriteOutcome::Written => Ok(()),
        TechCardWriteOutcome::AlreadyExists => Err(conflict(
            "the service already has a tech card; send expected-revision to replace it",
        )),
        TechCardWriteOutcome::Missing => Err(conflict("the service has no tech card")),
        TechCardWriteOutcome::RevisionMismatch => {
            Err(conflict("tech card changed since it was loaded"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(payment_object: &str, unit: &str, archived: bool) -> ProductForStock {
        ProductForStock {
            product_id: Uuid::nil(),
            name: "Шампунь".into(),
            unit: unit.into(),
            payment_object: payment_object.into(),
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
    fn only_stocked_goods_in_their_unit_go_on_stock_lines() {
        let one = Qty::from_milli(1000).unwrap();
        let half = Qty::from_milli(500).unwrap();
        assert!(
            check_stock_product(&product("commodity", "milliliter", false), half, true).is_ok()
        );
        assert!(rejection(check_stock_product(
            &product("service", "service_unit", false),
            one,
            false
        ))
        .contains("not stocked"));
        assert!(rejection(check_stock_product(
            &product("commodity", "piece", false),
            half,
            false
        ))
        .starts_with("invalid: lines.qty_milli"));
        assert!(rejection(check_stock_product(
            &product("excise", "piece", true),
            one,
            true
        ))
        .contains("archived"));
        assert!(check_stock_product(&product("excise", "piece", true), one, false).is_ok());
    }

    #[test]
    fn quantities_print_as_units() {
        assert_eq!(units(2_000), "2");
        assert_eq!(units(1_500), "1.5");
        assert_eq!(units(-30), "-0.03");
    }

    #[test]
    fn stored_sources_are_known() {
        assert_eq!(source_name("consumable").unwrap(), "consumable");
        assert!(source_name("gift").is_err());
    }

    #[test]
    fn tech_card_outcomes_map_to_conflicts() {
        assert!(tech_card_outcome(TechCardWriteOutcome::Written).is_ok());
        for outcome in [
            TechCardWriteOutcome::AlreadyExists,
            TechCardWriteOutcome::Missing,
            TechCardWriteOutcome::RevisionMismatch,
        ] {
            assert!(matches!(
                tech_card_outcome(outcome),
                Err(IngestError::CanvasConflict(_))
            ));
        }
    }
}
