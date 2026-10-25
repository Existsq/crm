//! CRM sale documents projection (migration 0053, kinds 42104 and 42105).
//!
//! The relay validates a sale against the rows read here (share-locked inside
//! the event-write transaction), computes every amount with `buzz_crm`, and
//! inserts the sale under the space row lock taken by its caller, so sale
//! numbers are gap-free and sequential per space.

use buzz_datastore_tracing::datastore_span;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Row as _, Transaction};
use uuid::Uuid;

use super::hex_bytes;
use crate::{error::Result, CommunityId, Db};

/// Largest page `list_sales` returns.
pub const MAX_SALE_PAGE: i64 = 200;

/// Catalog facts a sale line needs, read under a share lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductForSale {
    /// Product id.
    pub product_id: Uuid,
    /// Current name (snapshotted on the line).
    pub name: String,
    /// Unit wire name.
    pub unit: String,
    /// VAT wire name.
    pub vat: String,
    /// Whether the product is archived.
    pub archived: bool,
}

/// One computed sale line to insert.
#[derive(Debug, Clone)]
pub struct SaleLineInsert<'a> {
    /// Product sold.
    pub product_id: Uuid,
    /// Product name at sale time.
    pub product_name: &'a str,
    /// Unit wire name at sale time.
    pub unit: &'a str,
    /// VAT wire name at sale time.
    pub vat: &'a str,
    /// Quantity in thousandths.
    pub qty_milli: i64,
    /// Price per unit in kopecks.
    pub price_minor: i64,
    /// Line discount in kopecks.
    pub discount_minor: i64,
    /// Line amount after discount in kopecks.
    pub amount_minor: i64,
}

/// A validated, computed sale to insert.
#[derive(Debug, Clone)]
pub struct SaleInsert<'a> {
    /// Sale id (`crm-id` tag).
    pub sale_id: Uuid,
    /// Space channel.
    pub space_channel_id: Uuid,
    /// Warehouse the goods leave from.
    pub warehouse_id: Uuid,
    /// Buyer, if known.
    pub customer_id: Option<Uuid>,
    /// Payment wire name.
    pub payment: &'a str,
    /// Total after discounts.
    pub total_minor: i64,
    /// Sum of line discounts.
    pub discount_minor: i64,
    /// Note.
    pub note: Option<&'a str>,
    /// Sale time (the event's `created_at`).
    pub sold_at: DateTime<Utc>,
    /// The kind 42104 event id.
    pub event_id: &'a [u8],
    /// Event author.
    pub actor: &'a [u8],
    /// Lines in submitted order.
    pub lines: &'a [SaleLineInsert<'a>],
    /// Payment parts `(method, kopecks)`, one per method.
    pub payments: &'a [(&'a str, i64)],
    /// Cash handed over by the customer.
    pub cash_tendered_minor: Option<i64>,
}

/// Outcome of voiding a sale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaleVoidOutcome {
    /// The sale is now voided.
    Voided,
    /// No such sale in this space.
    Missing,
    /// The sale was voided before.
    AlreadyVoided,
}

/// A sale line as served by `GET /crm/sales/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmSaleLineRecord {
    /// Product sold.
    pub product_id: Uuid,
    /// Product name at sale time.
    pub product_name: String,
    /// Unit at sale time.
    pub unit: String,
    /// VAT at sale time.
    pub vat: String,
    /// Quantity in thousandths.
    pub qty_milli: i64,
    /// Price per unit in kopecks.
    pub price_minor: i64,
    /// Line discount in kopecks.
    pub discount_minor: i64,
    /// Amount after discount in kopecks.
    pub amount_minor: i64,
}

/// A sale as served by `GET /crm/sales`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmSaleRecord {
    /// Sale id.
    #[serde(rename = "id")]
    pub sale_id: Uuid,
    /// Sequential number within the space.
    pub number: i64,
    /// Warehouse id.
    pub warehouse_id: Uuid,
    /// Warehouse name (current).
    pub warehouse_name: String,
    /// Customer id.
    pub customer_id: Option<Uuid>,
    /// Customer name (current).
    pub customer_name: Option<String>,
    /// Payment wire name.
    pub payment: String,
    /// Total after discounts.
    pub total_minor: i64,
    /// Sum of line discounts.
    pub discount_minor: i64,
    /// Number of lines.
    pub line_count: i16,
    /// Note.
    pub note: Option<String>,
    /// Sale time.
    pub sold_at: DateTime<Utc>,
    /// Pubkey of the seller (event author).
    #[serde(serialize_with = "hex_bytes")]
    pub created_by: Vec<u8>,
    /// Kind 42104 event id.
    #[serde(serialize_with = "hex_bytes")]
    pub event_id: Vec<u8>,
    /// Void details, when the sale is voided.
    pub void: Option<CrmSaleVoid>,
    /// Payment parts, largest first.
    pub payments: Vec<CrmSalePayment>,
    /// Cash handed over by the customer.
    pub cash_tendered_minor: Option<i64>,
    /// Fiscal receipts printed for the sale (`sell`) and its return.
    pub fiscal: CrmSaleFiscal,
    /// Lines; filled only by [`get_sale`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<CrmSaleLineRecord>>,
}

/// One payment part of a sale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct CrmSalePayment {
    /// Method wire name.
    pub method: String,
    /// Amount, kopecks.
    pub amount_minor: i64,
}

/// What the ККМ returned for one receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct CrmFiscalRecord {
    /// Fiscal document number (ФД).
    pub document_number: i64,
    /// Fiscal sign (ФП).
    pub document_sign: String,
    /// Receipt number in the shift.
    pub receipt_number: i64,
    /// Shift number.
    pub shift_number: i64,
    /// Fiscal storage number (ФН).
    pub fn_number: String,
    /// ККТ registration number.
    pub registration_number: Option<String>,
    /// Device date and time.
    pub document_datetime: String,
    /// Printed total, kopecks.
    pub total_minor: i64,
}

/// Receipts of a sale: the sale receipt and, after a void, the return.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct CrmSaleFiscal {
    /// Sale receipt.
    #[serde(default)]
    pub sell: Option<CrmFiscalRecord>,
    /// Return receipt.
    #[serde(default)]
    pub sell_return: Option<CrmFiscalRecord>,
}

/// Void details of a sale.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmSaleVoid {
    /// When the sale was voided.
    pub voided_at: DateTime<Utc>,
    /// Why.
    pub reason: String,
    /// Who voided it.
    #[serde(serialize_with = "hex_bytes")]
    pub voided_by: Vec<u8>,
}

/// Filters for [`list_sales`].
#[derive(Debug, Clone, Default)]
pub struct SaleQuery {
    /// Only sales of this customer.
    pub customer_id: Option<Uuid>,
    /// Only sales at or after this instant.
    pub from: Option<DateTime<Utc>>,
    /// Only sales before this instant.
    pub to: Option<DateTime<Utc>>,
    /// Include voided sales.
    pub include_voided: bool,
    /// Keyset cursor: the number of the last sale of the previous page.
    pub before: Option<i64>,
    /// Page size (clamped to 1..=[`MAX_SALE_PAGE`]).
    pub limit: i64,
}

/// One page of sales, newest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SalePage {
    /// Sales by descending number.
    pub sales: Vec<CrmSaleRecord>,
    /// Cursor for the next page, when more rows exist.
    pub next: Option<i64>,
}

/// Whether a sale with this id exists in the community (any space).
pub async fn sale_exists(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    sale_id: Uuid,
) -> Result<bool> {
    let found: Option<i32> =
        sqlx::query_scalar("SELECT 1 FROM crm_sales WHERE community_id = $1 AND sale_id = $2")
            .bind(community.as_uuid())
            .bind(sale_id)
            .fetch_optional(&mut **tx)
            .await?;
    Ok(found.is_some())
}

/// Whether the warehouse is archived (`None` when it is not in the space),
/// share-locked so it cannot be archived while the sale commits.
pub async fn warehouse_for_sale(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    warehouse_id: Uuid,
) -> Result<Option<bool>> {
    let archived: Option<bool> = sqlx::query_scalar(
        "SELECT archived_at IS NOT NULL FROM crm_warehouses \
         WHERE community_id = $1 AND space_channel_id = $2 AND warehouse_id = $3 FOR SHARE",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(warehouse_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(archived)
}

/// The space's products among `product_ids`, share-locked. Missing ids are
/// simply absent from the result.
pub async fn products_for_sale(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    product_ids: &[Uuid],
) -> Result<Vec<ProductForSale>> {
    let rows = sqlx::query(
        "SELECT product_id, name, unit, vat, archived_at IS NOT NULL AS archived \
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
            Ok(ProductForSale {
                product_id: row.try_get("product_id")?,
                name: row.try_get("name")?,
                unit: row.try_get("unit")?,
                vat: row.try_get("vat")?,
                archived: row.try_get("archived")?,
            })
        })
        .collect()
}

/// Insert the sale and its lines with the next number of the space.
///
/// The caller holds the space row lock (`FOR UPDATE`), which serializes
/// numbering.
pub async fn insert_sale(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    sale: &SaleInsert<'_>,
) -> Result<i64> {
    let number: i64 = sqlx::query_scalar(
        "INSERT INTO crm_sales \
             (community_id, sale_id, space_channel_id, number, warehouse_id, customer_id, \
              payment, total_minor, discount_minor, line_count, note, sold_at, event_id, \
              created_by, cash_tendered_minor) \
         SELECT $1, $2, $3, COALESCE(max(number), 0) + 1, $4, $5, $6, $7, $8, $9, $10, $11, \
                $12, $13, $14 \
         FROM crm_sales WHERE community_id = $1 AND space_channel_id = $3 \
         RETURNING number",
    )
    .bind(community.as_uuid())
    .bind(sale.sale_id)
    .bind(sale.space_channel_id)
    .bind(sale.warehouse_id)
    .bind(sale.customer_id)
    .bind(sale.payment)
    .bind(sale.total_minor)
    .bind(sale.discount_minor)
    .bind(sale.lines.len() as i16)
    .bind(sale.note)
    .bind(sale.sold_at)
    .bind(sale.event_id)
    .bind(sale.actor)
    .bind(sale.cash_tendered_minor)
    .fetch_one(&mut **tx)
    .await?;

    let methods: Vec<&str> = sale.payments.iter().map(|(method, _)| *method).collect();
    let amounts: Vec<i64> = sale.payments.iter().map(|(_, amount)| *amount).collect();
    sqlx::query(
        "INSERT INTO crm_sale_payments (community_id, sale_id, method, amount_minor) \
         SELECT $1, $2, method, amount FROM unnest($3::text[], $4::bigint[]) AS p(method, amount)",
    )
    .bind(community.as_uuid())
    .bind(sale.sale_id)
    .bind(&methods)
    .bind(&amounts)
    .execute(&mut **tx)
    .await?;

    let column =
        |f: fn(&SaleLineInsert<'_>) -> i64| -> Vec<i64> { sale.lines.iter().map(f).collect() };
    let product_ids: Vec<Uuid> = sale.lines.iter().map(|line| line.product_id).collect();
    let names: Vec<&str> = sale.lines.iter().map(|line| line.product_name).collect();
    let units: Vec<&str> = sale.lines.iter().map(|line| line.unit).collect();
    let vats: Vec<&str> = sale.lines.iter().map(|line| line.vat).collect();
    sqlx::query(
        "INSERT INTO crm_sale_lines \
             (community_id, sale_id, position, product_id, product_name, unit, vat, qty_milli, \
              price_minor, discount_minor, amount_minor) \
         SELECT $1, $2, (ord - 1)::smallint, product_id, product_name, unit, vat, qty_milli, \
                price_minor, discount_minor, amount_minor \
         FROM unnest($3::uuid[], $4::text[], $5::text[], $6::text[], $7::bigint[], \
                     $8::bigint[], $9::bigint[], $10::bigint[]) WITH ORDINALITY \
              AS l(product_id, product_name, unit, vat, qty_milli, price_minor, discount_minor, \
                   amount_minor, ord)",
    )
    .bind(community.as_uuid())
    .bind(sale.sale_id)
    .bind(&product_ids)
    .bind(&names)
    .bind(&units)
    .bind(&vats)
    .bind(column(|line| line.qty_milli))
    .bind(column(|line| line.price_minor))
    .bind(column(|line| line.discount_minor))
    .bind(column(|line| line.amount_minor))
    .execute(&mut **tx)
    .await?;
    Ok(number)
}

/// Void a sale of the space (сторно), once.
pub async fn void_sale(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    sale_id: Uuid,
    void_event_id: &[u8],
    reason: &str,
    actor: &[u8],
) -> Result<SaleVoidOutcome> {
    let voided: Option<bool> = sqlx::query_scalar(
        "SELECT voided_at IS NOT NULL FROM crm_sales \
         WHERE community_id = $1 AND space_channel_id = $2 AND sale_id = $3 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(sale_id)
    .fetch_optional(&mut **tx)
    .await?;
    match voided {
        None => return Ok(SaleVoidOutcome::Missing),
        Some(true) => return Ok(SaleVoidOutcome::AlreadyVoided),
        Some(false) => {}
    }
    sqlx::query(
        "UPDATE crm_sales SET voided_at = now(), void_event_id = $3, void_reason = $4, \
                voided_by = $5 \
         WHERE community_id = $1 AND sale_id = $2",
    )
    .bind(community.as_uuid())
    .bind(sale_id)
    .bind(void_event_id)
    .bind(reason)
    .bind(actor)
    .execute(&mut **tx)
    .await?;
    Ok(SaleVoidOutcome::Voided)
}

macro_rules! sale_columns {
    () => {
        "s.sale_id, s.number, s.warehouse_id, w.name AS warehouse_name, s.customer_id, \
         p.name AS customer_name, s.payment, s.total_minor, s.discount_minor, s.line_count, \
         s.note, s.sold_at, s.created_by, s.event_id, s.voided_at, s.void_reason, s.voided_by, \
         s.cash_tendered_minor, \
         COALESCE((SELECT jsonb_agg(jsonb_build_object('method', sp.method, \
                                                       'amount_minor', sp.amount_minor) \
                                    ORDER BY sp.amount_minor DESC, sp.method) \
                   FROM crm_sale_payments sp \
                   WHERE sp.community_id = s.community_id AND sp.sale_id = s.sale_id), \
                  '[]'::jsonb) AS payments, \
         COALESCE((SELECT jsonb_object_agg(fr.operation, jsonb_build_object( \
                       'document_number', fr.document_number, \
                       'document_sign', fr.document_sign, \
                       'receipt_number', fr.receipt_number, \
                       'shift_number', fr.shift_number, \
                       'fn_number', fr.fn_number, \
                       'registration_number', fr.registration_number, \
                       'document_datetime', fr.document_datetime, \
                       'total_minor', fr.total_minor)) \
                   FROM crm_fiscal_receipts fr \
                   WHERE fr.community_id = s.community_id AND fr.sale_id = s.sale_id), \
                  '{}'::jsonb) AS fiscal \
         FROM crm_sales s \
         JOIN crm_warehouses w ON w.community_id = s.community_id \
                              AND w.warehouse_id = s.warehouse_id \
         LEFT JOIN crm_parties p ON p.community_id = s.community_id \
                                AND p.party_id = s.customer_id"
    };
}

/// One page of a space's sales, newest (highest number) first.
pub async fn list_sales(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    query: &SaleQuery,
) -> Result<SalePage> {
    let limit = query.limit.clamp(1, MAX_SALE_PAGE);
    let sql = concat!(
        "SELECT ",
        sale_columns!(),
        " WHERE s.community_id = $1 AND s.space_channel_id = $2 \
           AND ($3 OR s.voided_at IS NULL) \
           AND ($4::uuid IS NULL OR s.customer_id = $4) \
           AND ($5::timestamptz IS NULL OR s.sold_at >= $5) \
           AND ($6::timestamptz IS NULL OR s.sold_at < $6) \
           AND ($7::bigint IS NULL OR s.number < $7) \
         ORDER BY s.number DESC \
         LIMIT $8"
    );
    let rows = sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(space_channel_id)
        .bind(query.include_voided)
        .bind(query.customer_id)
        .bind(query.from)
        .bind(query.to)
        .bind(query.before)
        .bind(limit + 1)
        .fetch_all(pool)
        .await?;
    let mut sales = rows
        .into_iter()
        .map(sale_from_row)
        .collect::<Result<Vec<_>>>()?;
    let next = if sales.len() as i64 > limit {
        sales.truncate(limit as usize);
        sales.last().map(|sale| sale.number)
    } else {
        None
    };
    Ok(SalePage { sales, next })
}

/// One sale of the space with its lines.
pub async fn get_sale(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    sale_id: Uuid,
) -> Result<Option<CrmSaleRecord>> {
    let sql = concat!(
        "SELECT ",
        sale_columns!(),
        " WHERE s.community_id = $1 AND s.space_channel_id = $2 AND s.sale_id = $3"
    );
    let Some(row) = sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(space_channel_id)
        .bind(sale_id)
        .fetch_optional(pool)
        .await?
    else {
        return Ok(None);
    };
    let mut sale = sale_from_row(row)?;
    let lines = sqlx::query(
        "SELECT product_id, product_name, unit, vat, qty_milli, price_minor, discount_minor, \
                amount_minor \
         FROM crm_sale_lines WHERE community_id = $1 AND sale_id = $2 ORDER BY position",
    )
    .bind(community.as_uuid())
    .bind(sale_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(CrmSaleLineRecord {
            product_id: row.try_get("product_id")?,
            product_name: row.try_get("product_name")?,
            unit: row.try_get("unit")?,
            vat: row.try_get("vat")?,
            qty_milli: row.try_get("qty_milli")?,
            price_minor: row.try_get("price_minor")?,
            discount_minor: row.try_get("discount_minor")?,
            amount_minor: row.try_get("amount_minor")?,
        })
    })
    .collect::<Result<Vec<_>>>()?;
    sale.lines = Some(lines);
    Ok(Some(sale))
}

fn sale_from_row(row: sqlx::postgres::PgRow) -> Result<CrmSaleRecord> {
    let voided_at: Option<DateTime<Utc>> = row.try_get("voided_at")?;
    let void = match voided_at {
        Some(voided_at) => Some(CrmSaleVoid {
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
    Ok(CrmSaleRecord {
        sale_id: row.try_get("sale_id")?,
        number: row.try_get("number")?,
        warehouse_id: row.try_get("warehouse_id")?,
        warehouse_name: row.try_get("warehouse_name")?,
        customer_id: row.try_get("customer_id")?,
        customer_name: row.try_get("customer_name")?,
        payment: row.try_get("payment")?,
        total_minor: row.try_get("total_minor")?,
        discount_minor: row.try_get("discount_minor")?,
        line_count: row.try_get("line_count")?,
        note: row.try_get("note")?,
        sold_at: row.try_get("sold_at")?,
        created_by: row.try_get("created_by")?,
        event_id: row.try_get("event_id")?,
        void,
        payments: serde_json::from_value(row.try_get("payments")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
        cash_tendered_minor: row.try_get("cash_tendered_minor")?,
        fiscal: serde_json::from_value(row.try_get("fiscal")?)
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
        lines: None,
    })
}

impl Db {
    /// One page of CRM sales.
    #[datastore_span(name = "crm_list_sales", system = "postgresql")]
    pub async fn crm_list_sales(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        query: &SaleQuery,
    ) -> Result<SalePage> {
        list_sales(&self.pool, community, space_channel_id, query).await
    }

    /// One CRM sale with its lines.
    #[datastore_span(name = "crm_get_sale", system = "postgresql")]
    pub async fn crm_get_sale(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        sale_id: Uuid,
    ) -> Result<Option<CrmSaleRecord>> {
        get_sale(&self.pool, community, space_channel_id, sale_id).await
    }
}

#[cfg(test)]
mod postgres_tests;
