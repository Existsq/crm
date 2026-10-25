//! CRM product catalog projection (migration 0051, kind 42102).
//!
//! Writes run inside the caller's event-write transaction (see the parent
//! module). SKU and barcode uniqueness are enforced by unique indexes; the
//! writer checks them first so the rejection can name the duplicate value, and
//! maps a unique violation from a concurrent writer to the same outcome.

use buzz_datastore_tracing::datastore_span;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Row as _, Transaction};
use uuid::Uuid;

use super::hex_bytes;
use crate::{error::Result, CommunityId, Db};

/// Largest page `list_products` returns.
pub const MAX_PRODUCT_PAGE: i64 = 200;

/// Validated product fields for one revision.
#[derive(Debug, Clone, Copy)]
pub struct ProductWrite<'a> {
    /// Product id (`crm-id` tag).
    pub product_id: Uuid,
    /// Space channel.
    pub space_channel_id: Uuid,
    /// Trimmed name.
    pub name: &'a str,
    /// Normalized SKU.
    pub sku: Option<&'a str>,
    /// Normalized, de-duplicated barcodes in display order.
    pub barcodes: &'a [String],
    /// Unit wire name.
    pub unit: &'a str,
    /// Price in kopecks.
    pub price_minor: i64,
    /// VAT wire name.
    pub vat: &'a str,
    /// Fiscal item type wire name.
    pub payment_object: &'a str,
    /// Optional category.
    pub category: Option<&'a str>,
    /// Low-stock threshold in thousandths.
    pub min_stock_milli: i64,
    /// Whether the goods need a marking code.
    pub requires_marking: bool,
    /// Archive the product.
    pub archived: bool,
    /// Event producing this revision.
    pub revision_event_id: &'a [u8],
    /// Author of the event (recorded as creator on create).
    pub actor: &'a [u8],
}

/// Outcome of a product write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductWriteOutcome {
    /// Inserted or updated.
    Written,
    /// Create for an id that already exists.
    AlreadyExists,
    /// Update for an id that does not exist in this space.
    Missing,
    /// Update whose `expected-revision` is stale.
    RevisionMismatch,
    /// Another product in the space has this SKU (`None` when a concurrent
    /// writer won the race and the value is not known).
    SkuTaken(Option<String>),
    /// Another product in the space has this barcode.
    BarcodeTaken(Option<String>),
}

/// A product row as served by `GET /crm/products`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmProductRecord {
    /// Product id.
    #[serde(rename = "id")]
    pub product_id: Uuid,
    /// Name.
    pub name: String,
    /// SKU.
    pub sku: Option<String>,
    /// Barcodes in display order.
    pub barcodes: Vec<String>,
    /// Unit wire name.
    pub unit: String,
    /// Price in kopecks.
    pub price_minor: i64,
    /// VAT wire name.
    pub vat: String,
    /// Fiscal item type wire name.
    pub payment_object: String,
    /// Category.
    pub category: Option<String>,
    /// Low-stock threshold in thousandths.
    pub min_stock_milli: i64,
    /// Whether the goods need a marking code.
    pub requires_marking: bool,
    /// Whether the product is archived.
    pub archived: bool,
    /// Id of the event that produced the current row.
    #[serde(rename = "revision", serialize_with = "hex_bytes")]
    pub revision_event_id: Vec<u8>,
    /// Last change.
    pub updated_at: DateTime<Utc>,
}

/// Filters for [`list_products`].
#[derive(Debug, Clone, Default)]
pub struct ProductQuery {
    /// Case-insensitive substring of the name, or prefix of the SKU.
    pub q: Option<String>,
    /// Exact barcode.
    pub barcode: Option<String>,
    /// Include archived products.
    pub include_archived: bool,
    /// Keyset cursor: the id of the last product of the previous page.
    pub after: Option<Uuid>,
    /// Page size (clamped to 1..=[`MAX_PRODUCT_PAGE`]).
    pub limit: i64,
}

/// One page of products.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProductPage {
    /// Products in `(lower(name), id)` order.
    pub products: Vec<CrmProductRecord>,
    /// Cursor for the next page, when more rows exist.
    pub next: Option<Uuid>,
}

/// Create (no `expected_revision`) or CAS-update a product and replace its
/// barcode set.
pub async fn write_product(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    write: ProductWrite<'_>,
    expected_revision: Option<&[u8]>,
) -> Result<ProductWriteOutcome> {
    let current = sqlx::query(
        "SELECT space_channel_id, revision_event_id FROM crm_products \
         WHERE community_id = $1 AND product_id = $2 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(write.product_id)
    .fetch_optional(&mut **tx)
    .await?;
    match (expected_revision, current) {
        (None, Some(_)) => return Ok(ProductWriteOutcome::AlreadyExists),
        (Some(_), None) => return Ok(ProductWriteOutcome::Missing),
        (Some(expected), Some(row)) => {
            let space: Uuid = row.try_get("space_channel_id")?;
            if space != write.space_channel_id {
                return Ok(ProductWriteOutcome::Missing);
            }
            let revision: Vec<u8> = row.try_get("revision_event_id")?;
            if revision != expected {
                return Ok(ProductWriteOutcome::RevisionMismatch);
            }
        }
        (None, None) => {}
    }

    if let Some(sku) = write.sku {
        let taken: Option<String> = sqlx::query_scalar(
            "SELECT sku FROM crm_products \
             WHERE community_id = $1 AND space_channel_id = $2 AND lower(sku) = lower($3) \
               AND product_id <> $4 \
             LIMIT 1",
        )
        .bind(community.as_uuid())
        .bind(write.space_channel_id)
        .bind(sku)
        .bind(write.product_id)
        .fetch_optional(&mut **tx)
        .await?;
        if taken.is_some() {
            return Ok(ProductWriteOutcome::SkuTaken(Some(sku.to_owned())));
        }
    }
    if !write.barcodes.is_empty() {
        let taken: Option<String> = sqlx::query_scalar(
            "SELECT barcode FROM crm_product_barcodes \
             WHERE community_id = $1 AND space_channel_id = $2 AND barcode = ANY($3) \
               AND product_id <> $4 \
             ORDER BY barcode LIMIT 1",
        )
        .bind(community.as_uuid())
        .bind(write.space_channel_id)
        .bind(write.barcodes)
        .bind(write.product_id)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(barcode) = taken {
            return Ok(ProductWriteOutcome::BarcodeTaken(Some(barcode)));
        }
    }

    let written = if expected_revision.is_none() {
        bind_product_fields(
            sqlx::query(
                "INSERT INTO crm_products \
                     (community_id, product_id, space_channel_id, name, sku, unit, price_minor, \
                      vat, payment_object, category, min_stock_milli, requires_marking, \
                      archived_at, revision_event_id, created_by) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, \
                         CASE WHEN $13 THEN now() END, $14, $15) \
                 ON CONFLICT (community_id, product_id) DO NOTHING",
            ),
            community,
            &write,
        )
        .bind(write.actor)
        .execute(&mut **tx)
        .await
    } else {
        bind_product_fields(
            sqlx::query(
                "UPDATE crm_products SET \
                     name = $4, sku = $5, unit = $6, price_minor = $7, vat = $8, \
                     payment_object = $9, category = $10, min_stock_milli = $11, \
                     requires_marking = $12, \
                     archived_at = CASE WHEN $13 THEN COALESCE(archived_at, now()) END, \
                     revision_event_id = $14, updated_at = now() \
                 WHERE community_id = $1 AND product_id = $2 AND space_channel_id = $3",
            ),
            community,
            &write,
        )
        .execute(&mut **tx)
        .await
    };
    let written = match written {
        Ok(result) => result.rows_affected(),
        Err(error) => return unique_violation(error, ProductWriteOutcome::SkuTaken(None)),
    };
    if written != 1 {
        // Only reachable for a create racing another create of the same id.
        return Ok(ProductWriteOutcome::AlreadyExists);
    }

    sqlx::query("DELETE FROM crm_product_barcodes WHERE community_id = $1 AND product_id = $2")
        .bind(community.as_uuid())
        .bind(write.product_id)
        .execute(&mut **tx)
        .await?;
    if !write.barcodes.is_empty() {
        let inserted = sqlx::query(
            "INSERT INTO crm_product_barcodes \
                 (community_id, space_channel_id, barcode, product_id, position) \
             SELECT $1, $2, code, $3, (ord - 1)::smallint \
             FROM unnest($4::text[]) WITH ORDINALITY AS codes(code, ord)",
        )
        .bind(community.as_uuid())
        .bind(write.space_channel_id)
        .bind(write.product_id)
        .bind(write.barcodes)
        .execute(&mut **tx)
        .await;
        if let Err(error) = inserted {
            return unique_violation(error, ProductWriteOutcome::BarcodeTaken(None));
        }
    }
    Ok(ProductWriteOutcome::Written)
}

type PgQuery<'q> = sqlx::query::Query<'q, Postgres, sqlx::postgres::PgArguments>;

/// Bind `$1..$14` shared by the product insert and update.
fn bind_product_fields<'q>(
    query: PgQuery<'q>,
    community: CommunityId,
    write: &ProductWrite<'q>,
) -> PgQuery<'q> {
    query
        .bind(community.as_uuid())
        .bind(write.product_id)
        .bind(write.space_channel_id)
        .bind(write.name)
        .bind(write.sku)
        .bind(write.unit)
        .bind(write.price_minor)
        .bind(write.vat)
        .bind(write.payment_object)
        .bind(write.category)
        .bind(write.min_stock_milli)
        .bind(write.requires_marking)
        .bind(write.archived)
        .bind(write.revision_event_id)
}

/// Map a unique violation raised by a concurrent writer to `outcome`; any
/// other error propagates. The transaction is aborted either way, and the
/// caller rolls it back.
fn unique_violation(
    error: sqlx::Error,
    outcome: ProductWriteOutcome,
) -> Result<ProductWriteOutcome> {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().as_deref() == Some("23505") {
            return Ok(outcome);
        }
    }
    Err(error.into())
}

/// Select list shared by the product reads (a macro so every statement stays a
/// `&'static str`).
macro_rules! product_columns {
    () => {
        "p.product_id, p.name, p.sku, p.unit, p.price_minor, p.vat, p.payment_object, \
         p.category, p.min_stock_milli, p.requires_marking, \
         p.archived_at IS NOT NULL AS archived, p.revision_event_id, p.updated_at, \
         COALESCE((SELECT array_agg(b.barcode ORDER BY b.position) FROM crm_product_barcodes b \
                   WHERE b.community_id = p.community_id AND b.product_id = p.product_id), \
                  ARRAY[]::text[]) AS barcodes"
    };
}

/// Escape `%`, `_` and `\` for a `LIKE` pattern.
pub(super) fn like_escape(raw: &str) -> String {
    let mut escaped = String::with_capacity(raw.len());
    for c in raw.chars() {
        if matches!(c, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// One page of a space's products ordered by `(lower(name), product_id)`.
pub async fn list_products(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    query: &ProductQuery,
) -> Result<ProductPage> {
    let limit = query.limit.clamp(1, MAX_PRODUCT_PAGE);
    let q = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(like_escape);
    let barcode = query
        .barcode
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty());
    // One static statement; absent filters bind NULL and drop out.
    let sql = concat!(
        "SELECT ",
        product_columns!(),
        " FROM crm_products p \
         WHERE p.community_id = $1 AND p.space_channel_id = $2 \
           AND ($3 OR p.archived_at IS NULL) \
           AND ($4::text IS NULL \
                OR p.name ILIKE '%' || $4 || '%' \
                OR lower(p.sku) LIKE lower($4) || '%') \
           AND ($5::text IS NULL OR EXISTS ( \
                SELECT 1 FROM crm_product_barcodes b \
                WHERE b.community_id = p.community_id AND b.space_channel_id = p.space_channel_id \
                  AND b.barcode = $5 AND b.product_id = p.product_id)) \
           AND ($6::uuid IS NULL OR (lower(p.name), p.product_id) > ( \
                SELECT lower(c.name), c.product_id FROM crm_products c \
                WHERE c.community_id = $1 AND c.product_id = $6)) \
         ORDER BY lower(p.name), p.product_id \
         LIMIT $7"
    );
    let rows = sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(space_channel_id)
        .bind(query.include_archived)
        .bind(q)
        .bind(barcode)
        .bind(query.after)
        .bind(limit + 1)
        .fetch_all(pool)
        .await?;
    let mut products = rows
        .into_iter()
        .map(product_from_row)
        .collect::<Result<Vec<_>>>()?;
    let next = if products.len() as i64 > limit {
        products.truncate(limit as usize);
        products.last().map(|p| p.product_id)
    } else {
        None
    };
    Ok(ProductPage { products, next })
}

/// One product of the space, archived or not.
pub async fn get_product(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    product_id: Uuid,
) -> Result<Option<CrmProductRecord>> {
    let sql = concat!(
        "SELECT ",
        product_columns!(),
        " FROM crm_products p \
         WHERE p.community_id = $1 AND p.space_channel_id = $2 AND p.product_id = $3"
    );
    let row = sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(space_channel_id)
        .bind(product_id)
        .fetch_optional(pool)
        .await?;
    row.map(product_from_row).transpose()
}

fn product_from_row(row: sqlx::postgres::PgRow) -> Result<CrmProductRecord> {
    Ok(CrmProductRecord {
        product_id: row.try_get("product_id")?,
        name: row.try_get("name")?,
        sku: row.try_get("sku")?,
        barcodes: row.try_get("barcodes")?,
        unit: row.try_get("unit")?,
        price_minor: row.try_get("price_minor")?,
        vat: row.try_get("vat")?,
        payment_object: row.try_get("payment_object")?,
        category: row.try_get("category")?,
        min_stock_milli: row.try_get("min_stock_milli")?,
        requires_marking: row.try_get("requires_marking")?,
        archived: row.try_get("archived")?,
        revision_event_id: row.try_get("revision_event_id")?,
        updated_at: row.try_get("updated_at")?,
    })
}

impl Db {
    /// One page of CRM products.
    #[datastore_span(name = "crm_list_products", system = "postgresql")]
    pub async fn crm_list_products(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        query: &ProductQuery,
    ) -> Result<ProductPage> {
        list_products(&self.pool, community, space_channel_id, query).await
    }

    /// One CRM product.
    #[datastore_span(name = "crm_get_product", system = "postgresql")]
    pub async fn crm_get_product(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        product_id: Uuid,
    ) -> Result<Option<CrmProductRecord>> {
        get_product(&self.pool, community, space_channel_id, product_id).await
    }
}

#[cfg(test)]
mod postgres_tests;
