//! CRM stock ledger and service tech cards (migration 0060, kinds 42104–42108).
//!
//! The relay computes every movement with `buzz_crm::stock` from the levels
//! read here `FOR UPDATE`, then writes the moves and the resulting levels in
//! one go. Callers hold the space row lock exclusively (stock documents and
//! sales both do), so levels of one space change strictly one document at a
//! time and stock document numbers are gap-free.

use std::collections::HashMap;

use buzz_datastore_tracing::datastore_span;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Row as _, Transaction};
use uuid::Uuid;

use super::hex_bytes;
use crate::{error::Result, CommunityId, Db};

/// Largest page of stock rows, moves or documents.
pub const MAX_STOCK_PAGE: i64 = 500;

/// Catalog facts a stock line needs, read under a share lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductForStock {
    /// Product id.
    pub product_id: Uuid,
    /// Current name (snapshotted on document lines).
    pub name: String,
    /// Unit wire name.
    pub unit: String,
    /// Fiscal type wire name (decides whether the product is stocked).
    pub payment_object: String,
    /// Whether the product is archived.
    pub archived: bool,
}

/// One consumable of a service, as read for a sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechCardConsumable {
    /// Service product.
    pub service_id: Uuid,
    /// Consumed product.
    pub product_id: Uuid,
    /// Quantity per service unit, thousandths.
    pub qty_milli: i64,
}

/// One movement to record, with the level it leaves behind.
#[derive(Debug, Clone)]
pub struct StockMoveWrite {
    /// Warehouse.
    pub warehouse_id: Uuid,
    /// Product.
    pub product_id: Uuid,
    /// `receipt`, `write_off`, `inventory`, `transfer`, `sale`, `consumable`.
    pub source: &'static str,
    /// Whether this move reverts an earlier one (a void).
    pub reversal: bool,
    /// Quantity change, thousandths.
    pub qty_delta_milli: i64,
    /// Value change, kopecks.
    pub value_delta_minor: i64,
    /// Quantity after the move.
    pub qty_after_milli: i64,
    /// Value after the move.
    pub value_after_minor: i64,
}

/// A recorded movement of a document, as read to revert it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockMoveOfDoc {
    /// Warehouse.
    pub warehouse_id: Uuid,
    /// Product.
    pub product_id: Uuid,
    /// Source wire name.
    pub source: String,
    /// Quantity change, thousandths.
    pub qty_delta_milli: i64,
    /// Value change, kopecks.
    pub value_delta_minor: i64,
}

/// One computed stock document line to insert.
#[derive(Debug, Clone)]
pub struct StockDocLineInsert<'a> {
    /// Product.
    pub product_id: Uuid,
    /// Product name at posting time.
    pub product_name: &'a str,
    /// Unit wire name at posting time.
    pub unit: &'a str,
    /// Quantity moved; for an inventory, the counted quantity.
    pub qty_milli: i64,
    /// Inventory: the book quantity before the count.
    pub book_qty_milli: Option<i64>,
    /// Purchase cost per unit.
    pub cost_minor: Option<i64>,
    /// Cost value moved (absolute).
    pub amount_minor: i64,
}

/// A validated, computed stock document to insert.
#[derive(Debug, Clone)]
pub struct StockDocInsert<'a> {
    /// Document id (`crm-id` tag).
    pub doc_id: Uuid,
    /// Space channel.
    pub space_channel_id: Uuid,
    /// Type wire name.
    pub doc_type: &'a str,
    /// Warehouse (the source of a transfer).
    pub warehouse_id: Uuid,
    /// Transfer destination.
    pub to_warehouse_id: Option<Uuid>,
    /// Receipt supplier.
    pub supplier_id: Option<Uuid>,
    /// Supplier's document number.
    pub reference: Option<&'a str>,
    /// Note.
    pub note: Option<&'a str>,
    /// Receipt: purchase cost; other types: value moved.
    pub total_minor: i64,
    /// Posting time (the event's `created_at`).
    pub posted_at: DateTime<Utc>,
    /// The kind 42106 event id.
    pub event_id: &'a [u8],
    /// Event author.
    pub actor: &'a [u8],
    /// Lines in submitted order.
    pub lines: &'a [StockDocLineInsert<'a>],
}

/// Outcome of voiding a stock document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockDocVoidOutcome {
    /// The document is now voided.
    Voided,
    /// No such document in this space.
    Missing,
    /// The document was voided before.
    AlreadyVoided,
}

/// Outcome of a tech card write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TechCardWriteOutcome {
    /// Inserted or replaced.
    Written,
    /// Create for a service that already has a card.
    AlreadyExists,
    /// Update for a service without a card.
    Missing,
    /// Update whose `expected-revision` is stale.
    RevisionMismatch,
}

/// The space's `allow_negative_stock` setting.
pub async fn allows_negative_stock(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
) -> Result<bool> {
    let allowed: Option<bool> = sqlx::query_scalar(
        "SELECT (settings->>'allow_negative_stock')::boolean FROM crm_spaces \
         WHERE community_id = $1 AND channel_id = $2",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .fetch_optional(&mut **tx)
    .await?
    .flatten();
    Ok(allowed.unwrap_or(false))
}

/// The space's products among `product_ids`, share-locked. Missing ids are
/// absent from the result.
pub async fn products_for_stock(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    product_ids: &[Uuid],
) -> Result<Vec<ProductForStock>> {
    let rows = sqlx::query(
        "SELECT product_id, name, unit, payment_object, archived_at IS NOT NULL AS archived \
         FROM crm_products \
         WHERE community_id = $1 AND space_channel_id = $2 AND product_id = ANY($3) \
         ORDER BY product_id FOR SHARE",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(product_ids)
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(ProductForStock {
                product_id: row.try_get("product_id")?,
                name: row.try_get("name")?,
                unit: row.try_get("unit")?,
                payment_object: row.try_get("payment_object")?,
                archived: row.try_get("archived")?,
            })
        })
        .collect()
}

/// Whether the warehouse is archived (`None` when it is not in the space),
/// share-locked.
pub async fn warehouse_for_stock(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    warehouse_id: Uuid,
) -> Result<Option<bool>> {
    super::sales::warehouse_for_sale(tx, community, space_channel_id, warehouse_id).await
}

/// Lock the levels of `pairs` (`(warehouse, product)`), creating empty rows
/// for pairs never stocked. Returns `(qty_milli, value_minor)` per pair.
pub async fn lock_levels(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    pairs: &[(Uuid, Uuid)],
) -> Result<HashMap<(Uuid, Uuid), (i64, i64)>> {
    if pairs.is_empty() {
        return Ok(HashMap::new());
    }
    let mut sorted = pairs.to_vec();
    sorted.sort();
    sorted.dedup();
    let warehouses: Vec<Uuid> = sorted.iter().map(|(w, _)| *w).collect();
    let products: Vec<Uuid> = sorted.iter().map(|(_, p)| *p).collect();
    sqlx::query(
        "INSERT INTO crm_stock (community_id, warehouse_id, product_id, space_channel_id) \
         SELECT $1, w, p, $2 FROM unnest($3::uuid[], $4::uuid[]) AS s(w, p) \
         ON CONFLICT DO NOTHING",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(&warehouses)
    .bind(&products)
    .execute(&mut **tx)
    .await?;
    // Sorted key order keeps lock acquisition deadlock-free.
    let rows = sqlx::query(
        "SELECT st.warehouse_id, st.product_id, st.qty_milli, st.value_minor \
         FROM crm_stock st \
         JOIN unnest($2::uuid[], $3::uuid[]) AS s(w, p) \
           ON st.warehouse_id = s.w AND st.product_id = s.p \
         WHERE st.community_id = $1 \
         ORDER BY st.warehouse_id, st.product_id FOR UPDATE OF st",
    )
    .bind(community.as_uuid())
    .bind(&warehouses)
    .bind(&products)
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok((
                (row.try_get("warehouse_id")?, row.try_get("product_id")?),
                (row.try_get("qty_milli")?, row.try_get("value_minor")?),
            ))
        })
        .collect()
}

/// Record `moves` for document `doc_id` and store the levels they leave
/// behind (the last move of each pair wins).
pub async fn write_moves(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    doc_id: Uuid,
    moved_at: DateTime<Utc>,
    event_id: &[u8],
    moves: &[StockMoveWrite],
) -> Result<()> {
    if moves.is_empty() {
        return Ok(());
    }
    let column = |f: fn(&StockMoveWrite) -> i64| -> Vec<i64> { moves.iter().map(f).collect() };
    let warehouses: Vec<Uuid> = moves.iter().map(|m| m.warehouse_id).collect();
    let products: Vec<Uuid> = moves.iter().map(|m| m.product_id).collect();
    let sources: Vec<&str> = moves.iter().map(|m| m.source).collect();
    let reversals: Vec<bool> = moves.iter().map(|m| m.reversal).collect();
    sqlx::query(
        "INSERT INTO crm_stock_moves \
             (community_id, space_channel_id, warehouse_id, product_id, source, doc_id, \
              reversal, qty_delta_milli, value_delta_minor, qty_after_milli, moved_at, event_id) \
         SELECT $1, $2, w, p, src, $3, rev, dq, dv, qa, $4, $5 \
         FROM unnest($6::uuid[], $7::uuid[], $8::text[], $9::bool[], $10::bigint[], \
                     $11::bigint[], $12::bigint[]) WITH ORDINALITY \
              AS m(w, p, src, rev, dq, dv, qa, ord) \
         ORDER BY ord",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(doc_id)
    .bind(moved_at)
    .bind(event_id)
    .bind(&warehouses)
    .bind(&products)
    .bind(&sources)
    .bind(&reversals)
    .bind(column(|m| m.qty_delta_milli))
    .bind(column(|m| m.value_delta_minor))
    .bind(column(|m| m.qty_after_milli))
    .execute(&mut **tx)
    .await?;

    let mut last: HashMap<(Uuid, Uuid), (i64, i64)> = HashMap::new();
    for m in moves {
        last.insert(
            (m.warehouse_id, m.product_id),
            (m.qty_after_milli, m.value_after_minor),
        );
    }
    let mut levels: Vec<_> = last.into_iter().collect();
    levels.sort();
    let warehouses: Vec<Uuid> = levels.iter().map(|((w, _), _)| *w).collect();
    let products: Vec<Uuid> = levels.iter().map(|((_, p), _)| *p).collect();
    let qtys: Vec<i64> = levels.iter().map(|(_, (q, _))| *q).collect();
    let values: Vec<i64> = levels.iter().map(|(_, (_, v))| *v).collect();
    sqlx::query(
        "UPDATE crm_stock st SET qty_milli = s.q, value_minor = s.v, updated_at = now() \
         FROM unnest($2::uuid[], $3::uuid[], $4::bigint[], $5::bigint[]) AS s(w, p, q, v) \
         WHERE st.community_id = $1 AND st.warehouse_id = s.w AND st.product_id = s.p",
    )
    .bind(community.as_uuid())
    .bind(&warehouses)
    .bind(&products)
    .bind(&qtys)
    .bind(&values)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The forward (non-reversal) moves of a document or sale, in posting order.
pub async fn moves_of_doc(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    doc_id: Uuid,
) -> Result<Vec<StockMoveOfDoc>> {
    let rows = sqlx::query(
        "SELECT warehouse_id, product_id, source, qty_delta_milli, value_delta_minor \
         FROM crm_stock_moves \
         WHERE community_id = $1 AND doc_id = $2 AND NOT reversal ORDER BY move_id",
    )
    .bind(community.as_uuid())
    .bind(doc_id)
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StockMoveOfDoc {
                warehouse_id: row.try_get("warehouse_id")?,
                product_id: row.try_get("product_id")?,
                source: row.try_get("source")?,
                qty_delta_milli: row.try_get("qty_delta_milli")?,
                value_delta_minor: row.try_get("value_delta_minor")?,
            })
        })
        .collect()
}

/// Whether a stock document with this id exists in the community.
pub async fn stock_doc_exists(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    doc_id: Uuid,
) -> Result<bool> {
    let found: Option<i32> =
        sqlx::query_scalar("SELECT 1 FROM crm_stock_docs WHERE community_id = $1 AND doc_id = $2")
            .bind(community.as_uuid())
            .bind(doc_id)
            .fetch_optional(&mut **tx)
            .await?;
    Ok(found.is_some())
}

/// Insert the document and its lines with the next number of the space.
/// The caller holds the space row lock (`FOR UPDATE`).
pub async fn insert_stock_doc(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    doc: &StockDocInsert<'_>,
) -> Result<i64> {
    let number: i64 = sqlx::query_scalar(
        "INSERT INTO crm_stock_docs \
             (community_id, doc_id, space_channel_id, number, doc_type, warehouse_id, \
              to_warehouse_id, supplier_id, reference, note, line_count, total_minor, \
              posted_at, event_id, created_by) \
         SELECT $1, $2, $3, COALESCE(max(number), 0) + 1, $4, $5, $6, $7, $8, $9, $10, $11, \
                $12, $13, $14 \
         FROM crm_stock_docs WHERE community_id = $1 AND space_channel_id = $3 \
         RETURNING number",
    )
    .bind(community.as_uuid())
    .bind(doc.doc_id)
    .bind(doc.space_channel_id)
    .bind(doc.doc_type)
    .bind(doc.warehouse_id)
    .bind(doc.to_warehouse_id)
    .bind(doc.supplier_id)
    .bind(doc.reference)
    .bind(doc.note)
    .bind(doc.lines.len() as i16)
    .bind(doc.total_minor)
    .bind(doc.posted_at)
    .bind(doc.event_id)
    .bind(doc.actor)
    .fetch_one(&mut **tx)
    .await?;

    let lines = doc.lines;
    let product_ids: Vec<Uuid> = lines.iter().map(|l| l.product_id).collect();
    let names: Vec<&str> = lines.iter().map(|l| l.product_name).collect();
    let units: Vec<&str> = lines.iter().map(|l| l.unit).collect();
    let qtys: Vec<i64> = lines.iter().map(|l| l.qty_milli).collect();
    let books: Vec<Option<i64>> = lines.iter().map(|l| l.book_qty_milli).collect();
    let costs: Vec<Option<i64>> = lines.iter().map(|l| l.cost_minor).collect();
    let amounts: Vec<i64> = lines.iter().map(|l| l.amount_minor).collect();
    sqlx::query(
        "INSERT INTO crm_stock_doc_lines \
             (community_id, doc_id, position, product_id, product_name, unit, qty_milli, \
              book_qty_milli, cost_minor, amount_minor) \
         SELECT $1, $2, (ord - 1)::smallint, product_id, product_name, unit, qty_milli, \
                book_qty_milli, cost_minor, amount_minor \
         FROM unnest($3::uuid[], $4::text[], $5::text[], $6::bigint[], $7::bigint[], \
                     $8::bigint[], $9::bigint[]) WITH ORDINALITY \
              AS l(product_id, product_name, unit, qty_milli, book_qty_milli, cost_minor, \
                   amount_minor, ord)",
    )
    .bind(community.as_uuid())
    .bind(doc.doc_id)
    .bind(&product_ids)
    .bind(&names)
    .bind(&units)
    .bind(&qtys)
    .bind(&books)
    .bind(&costs)
    .bind(&amounts)
    .execute(&mut **tx)
    .await?;
    Ok(number)
}

/// Mark a stock document of the space voided, once.
pub async fn void_stock_doc(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    doc_id: Uuid,
    void_event_id: &[u8],
    reason: &str,
    actor: &[u8],
) -> Result<StockDocVoidOutcome> {
    let voided: Option<bool> = sqlx::query_scalar(
        "SELECT voided_at IS NOT NULL FROM crm_stock_docs \
         WHERE community_id = $1 AND space_channel_id = $2 AND doc_id = $3 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(doc_id)
    .fetch_optional(&mut **tx)
    .await?;
    match voided {
        None => return Ok(StockDocVoidOutcome::Missing),
        Some(true) => return Ok(StockDocVoidOutcome::AlreadyVoided),
        Some(false) => {}
    }
    sqlx::query(
        "UPDATE crm_stock_docs SET voided_at = now(), void_event_id = $3, void_reason = $4, \
                voided_by = $5 \
         WHERE community_id = $1 AND doc_id = $2",
    )
    .bind(community.as_uuid())
    .bind(doc_id)
    .bind(void_event_id)
    .bind(reason)
    .bind(actor)
    .execute(&mut **tx)
    .await?;
    Ok(StockDocVoidOutcome::Voided)
}

/// The consumables of the given services.
pub async fn consumables_for_services(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    service_ids: &[Uuid],
) -> Result<Vec<TechCardConsumable>> {
    if service_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT service_id, product_id, qty_milli FROM crm_tech_card_items \
         WHERE community_id = $1 AND service_id = ANY($2) ORDER BY service_id, position",
    )
    .bind(community.as_uuid())
    .bind(service_ids)
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(TechCardConsumable {
                service_id: row.try_get("service_id")?,
                product_id: row.try_get("product_id")?,
                qty_milli: row.try_get("qty_milli")?,
            })
        })
        .collect()
}

/// Create (no `expected_revision`) or CAS-replace the tech card of a
/// service. The caller validated the service and the consumed products.
#[allow(clippy::too_many_arguments)]
pub async fn write_tech_card(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    service_id: Uuid,
    items: &[(Uuid, i64)],
    expected_revision: Option<&[u8]>,
    revision_event_id: &[u8],
    actor: &[u8],
) -> Result<TechCardWriteOutcome> {
    let current: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT revision_event_id FROM crm_tech_cards \
         WHERE community_id = $1 AND service_id = $2 AND space_channel_id = $3 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(service_id)
    .bind(space_channel_id)
    .fetch_optional(&mut **tx)
    .await?;
    match (expected_revision, current) {
        (None, Some(_)) => return Ok(TechCardWriteOutcome::AlreadyExists),
        (Some(_), None) => return Ok(TechCardWriteOutcome::Missing),
        (Some(expected), Some(revision)) if revision != expected => {
            return Ok(TechCardWriteOutcome::RevisionMismatch)
        }
        (Some(_), Some(_)) => {
            sqlx::query(
                "UPDATE crm_tech_cards SET revision_event_id = $3, updated_at = now() \
                 WHERE community_id = $1 AND service_id = $2",
            )
            .bind(community.as_uuid())
            .bind(service_id)
            .bind(revision_event_id)
            .execute(&mut **tx)
            .await?;
            sqlx::query(
                "DELETE FROM crm_tech_card_items WHERE community_id = $1 AND service_id = $2",
            )
            .bind(community.as_uuid())
            .bind(service_id)
            .execute(&mut **tx)
            .await?;
        }
        (None, None) => {
            let inserted = sqlx::query(
                "INSERT INTO crm_tech_cards \
                     (community_id, service_id, space_channel_id, revision_event_id, created_by) \
                 VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
            )
            .bind(community.as_uuid())
            .bind(service_id)
            .bind(space_channel_id)
            .bind(revision_event_id)
            .bind(actor)
            .execute(&mut **tx)
            .await?
            .rows_affected();
            if inserted == 0 {
                return Ok(TechCardWriteOutcome::AlreadyExists);
            }
        }
    }
    let products: Vec<Uuid> = items.iter().map(|(p, _)| *p).collect();
    let qtys: Vec<i64> = items.iter().map(|(_, q)| *q).collect();
    sqlx::query(
        "INSERT INTO crm_tech_card_items (community_id, service_id, position, product_id, qty_milli) \
         SELECT $1, $2, (ord - 1)::smallint, p, q \
         FROM unnest($3::uuid[], $4::bigint[]) WITH ORDINALITY AS i(p, q, ord)",
    )
    .bind(community.as_uuid())
    .bind(service_id)
    .bind(&products)
    .bind(&qtys)
    .execute(&mut **tx)
    .await?;
    Ok(TechCardWriteOutcome::Written)
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// One product's stock as served by `GET /crm/stock`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmStockRecord {
    /// Product id.
    pub product_id: Uuid,
    /// Current name.
    pub name: String,
    /// Article number.
    pub sku: Option<String>,
    /// Unit wire name.
    pub unit: String,
    /// Category.
    pub category: Option<String>,
    /// Sale price per unit, kopecks.
    pub price_minor: i64,
    /// Low-stock threshold.
    pub min_stock_milli: i64,
    /// Quantity on hand (summed over warehouses without a warehouse filter).
    pub qty_milli: i64,
    /// Cost value of the quantity on hand.
    pub value_minor: i64,
    /// Whether the product is archived.
    pub archived: bool,
}

/// Filters for [`list_stock`].
#[derive(Debug, Clone, Default)]
pub struct StockQuery {
    /// One warehouse; all warehouses when absent.
    pub warehouse_id: Option<Uuid>,
    /// Case-insensitive name/SKU substring.
    pub search: Option<String>,
    /// Only products at or below their minimum stock.
    pub low_only: bool,
}

/// Stock of the space's goods: every live stocked product, plus archived or
/// unstocked ones that still have stock, by name.
pub async fn list_stock(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    query: &StockQuery,
) -> Result<Vec<CrmStockRecord>> {
    let pattern = query.search.as_deref().map(|s| {
        format!(
            "%{}%",
            s.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    });
    let rows = sqlx::query(
        "SELECT p.product_id, p.name, p.sku, p.unit, p.category, p.price_minor, \
                p.min_stock_milli, p.archived_at IS NOT NULL AS archived, \
                COALESCE(s.qty, 0)::bigint AS qty_milli, COALESCE(s.value, 0)::bigint AS value_minor \
         FROM crm_products p \
         LEFT JOIN (SELECT product_id, sum(qty_milli) AS qty, sum(value_minor) AS value \
                    FROM crm_stock \
                    WHERE community_id = $1 AND space_channel_id = $2 \
                      AND ($3::uuid IS NULL OR warehouse_id = $3) \
                    GROUP BY product_id) s ON s.product_id = p.product_id \
         WHERE p.community_id = $1 AND p.space_channel_id = $2 \
           AND ((p.archived_at IS NULL AND p.payment_object IN ('commodity', 'excise')) \
                OR COALESCE(s.qty, 0) <> 0) \
           AND ($4::text IS NULL OR p.name ILIKE $4 OR p.sku ILIKE $4) \
           AND (NOT $5 OR COALESCE(s.qty, 0) <= p.min_stock_milli) \
         ORDER BY lower(p.name), p.product_id LIMIT $6",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(query.warehouse_id)
    .bind(pattern)
    .bind(query.low_only)
    .bind(MAX_STOCK_PAGE * 4)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(CrmStockRecord {
                product_id: row.try_get("product_id")?,
                name: row.try_get("name")?,
                sku: row.try_get("sku")?,
                unit: row.try_get("unit")?,
                category: row.try_get("category")?,
                price_minor: row.try_get("price_minor")?,
                min_stock_milli: row.try_get("min_stock_milli")?,
                qty_milli: row.try_get("qty_milli")?,
                value_minor: row.try_get("value_minor")?,
                archived: row.try_get("archived")?,
            })
        })
        .collect()
}

/// A movement as served by `GET /crm/stock/moves`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmStockMoveRecord {
    /// Journal id (keyset cursor).
    pub id: i64,
    /// Warehouse.
    pub warehouse_id: Uuid,
    /// Warehouse name (current).
    pub warehouse_name: String,
    /// Product.
    pub product_id: Uuid,
    /// Product name (current).
    pub product_name: String,
    /// Unit wire name.
    pub unit: String,
    /// Source wire name.
    pub source: String,
    /// Stock document or sale id.
    pub doc_id: Uuid,
    /// Its number (sale number for `sale`/`consumable`).
    pub doc_number: Option<i64>,
    /// Whether the move reverts a voided document.
    pub reversal: bool,
    /// Quantity change.
    pub qty_delta_milli: i64,
    /// Value change.
    pub value_delta_minor: i64,
    /// Quantity on the warehouse after the move.
    pub qty_after_milli: i64,
    /// When.
    pub moved_at: DateTime<Utc>,
}

/// Filters for [`list_moves`].
#[derive(Debug, Clone, Default)]
pub struct StockMoveQuery {
    /// One warehouse.
    pub warehouse_id: Option<Uuid>,
    /// One product.
    pub product_id: Option<Uuid>,
    /// Keyset cursor: the id of the last move of the previous page.
    pub before: Option<i64>,
    /// Page size (clamped to 1..=[`MAX_STOCK_PAGE`]).
    pub limit: i64,
}

/// One page of movements, newest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StockMovePage {
    /// Moves by descending id.
    pub moves: Vec<CrmStockMoveRecord>,
    /// Cursor for the next page.
    pub next: Option<i64>,
}

/// One page of the space's stock movements.
pub async fn list_moves(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    query: &StockMoveQuery,
) -> Result<StockMovePage> {
    let limit = query.limit.clamp(1, MAX_STOCK_PAGE);
    let rows = sqlx::query(
        "SELECT m.move_id, m.warehouse_id, w.name AS warehouse_name, m.product_id, \
                p.name AS product_name, p.unit, m.source, m.doc_id, \
                COALESCE(d.number, s.number) AS doc_number, m.reversal, m.qty_delta_milli, \
                m.value_delta_minor, m.qty_after_milli, m.moved_at \
         FROM crm_stock_moves m \
         JOIN crm_warehouses w ON w.community_id = m.community_id \
                              AND w.warehouse_id = m.warehouse_id \
         JOIN crm_products p ON p.community_id = m.community_id AND p.product_id = m.product_id \
         LEFT JOIN crm_stock_docs d ON d.community_id = m.community_id AND d.doc_id = m.doc_id \
         LEFT JOIN crm_sales s ON s.community_id = m.community_id AND s.sale_id = m.doc_id \
         WHERE m.community_id = $1 AND m.space_channel_id = $2 \
           AND ($3::uuid IS NULL OR m.warehouse_id = $3) \
           AND ($4::uuid IS NULL OR m.product_id = $4) \
           AND ($5::bigint IS NULL OR m.move_id < $5) \
         ORDER BY m.move_id DESC LIMIT $6",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(query.warehouse_id)
    .bind(query.product_id)
    .bind(query.before)
    .bind(limit + 1)
    .fetch_all(pool)
    .await?;
    let mut moves = rows
        .into_iter()
        .map(|row| {
            Ok(CrmStockMoveRecord {
                id: row.try_get("move_id")?,
                warehouse_id: row.try_get("warehouse_id")?,
                warehouse_name: row.try_get("warehouse_name")?,
                product_id: row.try_get("product_id")?,
                product_name: row.try_get("product_name")?,
                unit: row.try_get("unit")?,
                source: row.try_get("source")?,
                doc_id: row.try_get("doc_id")?,
                doc_number: row.try_get("doc_number")?,
                reversal: row.try_get("reversal")?,
                qty_delta_milli: row.try_get("qty_delta_milli")?,
                value_delta_minor: row.try_get("value_delta_minor")?,
                qty_after_milli: row.try_get("qty_after_milli")?,
                moved_at: row.try_get("moved_at")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let next = if moves.len() as i64 > limit {
        moves.truncate(limit as usize);
        moves.last().map(|m| m.id)
    } else {
        None
    };
    Ok(StockMovePage { moves, next })
}

/// A stock document line as served by `GET /crm/stock/docs/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmStockDocLineRecord {
    /// Product.
    pub product_id: Uuid,
    /// Name at posting time.
    pub product_name: String,
    /// Unit at posting time.
    pub unit: String,
    /// Quantity moved; for an inventory, the counted quantity.
    pub qty_milli: i64,
    /// Inventory: the book quantity before the count.
    pub book_qty_milli: Option<i64>,
    /// Purchase cost per unit.
    pub cost_minor: Option<i64>,
    /// Cost value moved.
    pub amount_minor: i64,
}

/// Void details of a stock document.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmStockDocVoid {
    /// When.
    pub voided_at: DateTime<Utc>,
    /// Why.
    pub reason: String,
    /// Who.
    #[serde(serialize_with = "hex_bytes")]
    pub voided_by: Vec<u8>,
}

/// A stock document as served by `GET /crm/stock/docs`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmStockDocRecord {
    /// Document id.
    #[serde(rename = "id")]
    pub doc_id: Uuid,
    /// Sequential number within the space.
    pub number: i64,
    /// Type wire name.
    #[serde(rename = "type")]
    pub doc_type: String,
    /// Warehouse.
    pub warehouse_id: Uuid,
    /// Warehouse name (current).
    pub warehouse_name: String,
    /// Transfer destination.
    pub to_warehouse_id: Option<Uuid>,
    /// Destination name (current).
    pub to_warehouse_name: Option<String>,
    /// Receipt supplier.
    pub supplier_id: Option<Uuid>,
    /// Supplier name (current).
    pub supplier_name: Option<String>,
    /// Supplier's document number.
    pub reference: Option<String>,
    /// Note.
    pub note: Option<String>,
    /// Number of lines.
    pub line_count: i16,
    /// Receipt: purchase cost; other types: cost value moved.
    pub total_minor: i64,
    /// Posting time.
    pub posted_at: DateTime<Utc>,
    /// Author pubkey.
    #[serde(serialize_with = "hex_bytes")]
    pub created_by: Vec<u8>,
    /// Kind 42106 event id.
    #[serde(serialize_with = "hex_bytes")]
    pub event_id: Vec<u8>,
    /// Void details.
    pub void: Option<CrmStockDocVoid>,
    /// Lines; filled only by [`get_stock_doc`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<CrmStockDocLineRecord>>,
}

/// Filters for [`list_stock_docs`].
#[derive(Debug, Clone, Default)]
pub struct StockDocQuery {
    /// One type.
    pub doc_type: Option<String>,
    /// Keyset cursor: the number of the last document of the previous page.
    pub before: Option<i64>,
    /// Page size (clamped to 1..=[`MAX_STOCK_PAGE`]).
    pub limit: i64,
}

/// One page of stock documents, newest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StockDocPage {
    /// Documents by descending number.
    pub docs: Vec<CrmStockDocRecord>,
    /// Cursor for the next page.
    pub next: Option<i64>,
}

macro_rules! doc_columns {
    () => {
        "d.doc_id, d.number, d.doc_type, d.warehouse_id, w.name AS warehouse_name, \
         d.to_warehouse_id, tw.name AS to_warehouse_name, d.supplier_id, \
         sp.name AS supplier_name, d.reference, d.note, d.line_count, d.total_minor, \
         d.posted_at, d.created_by, d.event_id, d.voided_at, d.void_reason, d.voided_by \
         FROM crm_stock_docs d \
         JOIN crm_warehouses w ON w.community_id = d.community_id \
                              AND w.warehouse_id = d.warehouse_id \
         LEFT JOIN crm_warehouses tw ON tw.community_id = d.community_id \
                                    AND tw.warehouse_id = d.to_warehouse_id \
         LEFT JOIN crm_parties sp ON sp.community_id = d.community_id \
                                 AND sp.party_id = d.supplier_id"
    };
}

/// One page of the space's stock documents.
pub async fn list_stock_docs(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    query: &StockDocQuery,
) -> Result<StockDocPage> {
    let limit = query.limit.clamp(1, MAX_STOCK_PAGE);
    let sql = concat!(
        "SELECT ",
        doc_columns!(),
        " WHERE d.community_id = $1 AND d.space_channel_id = $2 \
           AND ($3::text IS NULL OR d.doc_type = $3) \
           AND ($4::bigint IS NULL OR d.number < $4) \
         ORDER BY d.number DESC LIMIT $5"
    );
    let rows = sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(space_channel_id)
        .bind(query.doc_type.as_deref())
        .bind(query.before)
        .bind(limit + 1)
        .fetch_all(pool)
        .await?;
    let mut docs = rows
        .into_iter()
        .map(doc_from_row)
        .collect::<Result<Vec<_>>>()?;
    let next = if docs.len() as i64 > limit {
        docs.truncate(limit as usize);
        docs.last().map(|doc| doc.number)
    } else {
        None
    };
    Ok(StockDocPage { docs, next })
}

/// One stock document of the space with its lines.
pub async fn get_stock_doc(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    doc_id: Uuid,
) -> Result<Option<CrmStockDocRecord>> {
    let sql = concat!(
        "SELECT ",
        doc_columns!(),
        " WHERE d.community_id = $1 AND d.space_channel_id = $2 AND d.doc_id = $3"
    );
    let Some(row) = sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(space_channel_id)
        .bind(doc_id)
        .fetch_optional(pool)
        .await?
    else {
        return Ok(None);
    };
    let mut doc = doc_from_row(row)?;
    let lines = sqlx::query(
        "SELECT product_id, product_name, unit, qty_milli, book_qty_milli, cost_minor, \
                amount_minor \
         FROM crm_stock_doc_lines WHERE community_id = $1 AND doc_id = $2 ORDER BY position",
    )
    .bind(community.as_uuid())
    .bind(doc_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(CrmStockDocLineRecord {
            product_id: row.try_get("product_id")?,
            product_name: row.try_get("product_name")?,
            unit: row.try_get("unit")?,
            qty_milli: row.try_get("qty_milli")?,
            book_qty_milli: row.try_get("book_qty_milli")?,
            cost_minor: row.try_get("cost_minor")?,
            amount_minor: row.try_get("amount_minor")?,
        })
    })
    .collect::<Result<Vec<_>>>()?;
    doc.lines = Some(lines);
    Ok(Some(doc))
}

fn doc_from_row(row: sqlx::postgres::PgRow) -> Result<CrmStockDocRecord> {
    let voided_at: Option<DateTime<Utc>> = row.try_get("voided_at")?;
    let void = match voided_at {
        Some(voided_at) => Some(CrmStockDocVoid {
            voided_at,
            reason: row
                .try_get::<Option<String>, _>("void_reason")?
                .unwrap_or_default(),
            voided_by: row
                .try_get::<Option<Vec<u8>>, _>("voided_by")?
                .unwrap_or_default(),
        }),
        None => None,
    };
    Ok(CrmStockDocRecord {
        doc_id: row.try_get("doc_id")?,
        number: row.try_get("number")?,
        doc_type: row.try_get("doc_type")?,
        warehouse_id: row.try_get("warehouse_id")?,
        warehouse_name: row.try_get("warehouse_name")?,
        to_warehouse_id: row.try_get("to_warehouse_id")?,
        to_warehouse_name: row.try_get("to_warehouse_name")?,
        supplier_id: row.try_get("supplier_id")?,
        supplier_name: row.try_get("supplier_name")?,
        reference: row.try_get("reference")?,
        note: row.try_get("note")?,
        line_count: row.try_get("line_count")?,
        total_minor: row.try_get("total_minor")?,
        posted_at: row.try_get("posted_at")?,
        created_by: row.try_get("created_by")?,
        event_id: row.try_get("event_id")?,
        void,
        lines: None,
    })
}

/// A consumable as served by `GET /crm/tech-cards`.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct CrmTechCardItemRecord {
    /// Consumed product.
    pub product_id: Uuid,
    /// Its current name.
    pub product_name: String,
    /// Its unit.
    pub unit: String,
    /// Quantity per service unit.
    pub qty_milli: i64,
}

/// A tech card as served by `GET /crm/tech-cards`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmTechCardRecord {
    /// Service product.
    pub service_id: Uuid,
    /// Its current name.
    pub service_name: String,
    /// Consumables in card order.
    pub items: Vec<CrmTechCardItemRecord>,
    /// Id of the event that produced the current card.
    #[serde(rename = "revision", serialize_with = "hex_bytes")]
    pub revision_event_id: Vec<u8>,
    /// Last change.
    pub updated_at: DateTime<Utc>,
}

/// Every tech card of the space, by service name.
pub async fn list_tech_cards(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
) -> Result<Vec<CrmTechCardRecord>> {
    let rows = sqlx::query(
        "SELECT c.service_id, s.name AS service_name, c.revision_event_id, c.updated_at, \
                COALESCE((SELECT jsonb_agg(jsonb_build_object( \
                              'product_id', i.product_id, 'product_name', p.name, \
                              'unit', p.unit, 'qty_milli', i.qty_milli) ORDER BY i.position) \
                          FROM crm_tech_card_items i \
                          JOIN crm_products p ON p.community_id = i.community_id \
                                             AND p.product_id = i.product_id \
                          WHERE i.community_id = c.community_id \
                            AND i.service_id = c.service_id), '[]'::jsonb) AS items \
         FROM crm_tech_cards c \
         JOIN crm_products s ON s.community_id = c.community_id AND s.product_id = c.service_id \
         WHERE c.community_id = $1 AND c.space_channel_id = $2 \
         ORDER BY lower(s.name), c.service_id LIMIT $3",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(MAX_STOCK_PAGE)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(CrmTechCardRecord {
                service_id: row.try_get("service_id")?,
                service_name: row.try_get("service_name")?,
                items: serde_json::from_value(row.try_get("items")?)
                    .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
                revision_event_id: row.try_get("revision_event_id")?,
                updated_at: row.try_get("updated_at")?,
            })
        })
        .collect()
}

impl Db {
    /// Stock of the space's goods.
    #[datastore_span(name = "crm_list_stock", system = "postgresql")]
    pub async fn crm_list_stock(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        query: &StockQuery,
    ) -> Result<Vec<CrmStockRecord>> {
        list_stock(&self.pool, community, space_channel_id, query).await
    }

    /// One page of stock movements.
    #[datastore_span(name = "crm_list_stock_moves", system = "postgresql")]
    pub async fn crm_list_stock_moves(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        query: &StockMoveQuery,
    ) -> Result<StockMovePage> {
        list_moves(&self.pool, community, space_channel_id, query).await
    }

    /// One page of stock documents.
    #[datastore_span(name = "crm_list_stock_docs", system = "postgresql")]
    pub async fn crm_list_stock_docs(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        query: &StockDocQuery,
    ) -> Result<StockDocPage> {
        list_stock_docs(&self.pool, community, space_channel_id, query).await
    }

    /// One stock document with its lines.
    #[datastore_span(name = "crm_get_stock_doc", system = "postgresql")]
    pub async fn crm_get_stock_doc(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        doc_id: Uuid,
    ) -> Result<Option<CrmStockDocRecord>> {
        get_stock_doc(&self.pool, community, space_channel_id, doc_id).await
    }

    /// Every tech card of the space.
    #[datastore_span(name = "crm_list_tech_cards", system = "postgresql")]
    pub async fn crm_list_tech_cards(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
    ) -> Result<Vec<CrmTechCardRecord>> {
        list_tech_cards(&self.pool, community, space_channel_id).await
    }
}

#[cfg(test)]
mod postgres_tests;
