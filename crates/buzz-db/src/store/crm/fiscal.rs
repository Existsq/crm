//! Fiscal receipts and cash-register shifts (migration 0054, kinds 42120 and
//! 42121). Written inside the caller's event-write transaction.

use buzz_datastore_tracing::datastore_span;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Row as _, Transaction};
use uuid::Uuid;

use crate::{error::Result, CommunityId, Db};

/// A receipt to record.
#[derive(Debug, Clone)]
pub struct ReceiptWrite<'a> {
    /// Sale the receipt is for (`crm-id`).
    pub sale_id: Uuid,
    /// Space channel.
    pub space_channel_id: Uuid,
    /// `sell` or `sell_return`.
    pub operation: &'a str,
    /// Printed total, kopecks.
    pub total_minor: i64,
    /// Device task UUID.
    pub task_uuid: Uuid,
    /// Fiscal document number.
    pub document_number: i64,
    /// Fiscal sign.
    pub document_sign: &'a str,
    /// Receipt number in the shift.
    pub receipt_number: i64,
    /// Shift number.
    pub shift_number: i64,
    /// Fiscal storage number.
    pub fn_number: &'a str,
    /// ККТ registration number.
    pub registration_number: Option<&'a str>,
    /// Device date and time.
    pub document_datetime: &'a str,
    /// Kind 42120 event id.
    pub event_id: &'a [u8],
    /// Event author.
    pub actor: &'a [u8],
}

/// Outcome of recording a receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptOutcome {
    /// Recorded.
    Recorded,
    /// No such sale in this space.
    SaleMissing,
    /// This sale already has a receipt of this operation.
    AlreadyRecorded,
    /// The printed total differs from the sale total.
    TotalMismatch {
        /// Sale total, kopecks.
        expected: i64,
    },
    /// A return receipt for a sale that is not voided, or without a sale receipt.
    NotReturnable,
    /// Another receipt already has this ФН + document number.
    DocumentTaken,
}

/// Record the receipt the ККМ printed for a sale (or its return).
pub async fn record_receipt(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    receipt: &ReceiptWrite<'_>,
) -> Result<ReceiptOutcome> {
    let sale = sqlx::query(
        "SELECT total_minor, voided_at IS NOT NULL AS voided FROM crm_sales \
         WHERE community_id = $1 AND space_channel_id = $2 AND sale_id = $3 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(receipt.space_channel_id)
    .bind(receipt.sale_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(sale) = sale else {
        return Ok(ReceiptOutcome::SaleMissing);
    };
    let total: i64 = sale.try_get("total_minor")?;
    let voided: bool = sale.try_get("voided")?;
    let existing: Vec<String> = sqlx::query_scalar(
        "SELECT operation FROM crm_fiscal_receipts WHERE community_id = $1 AND sale_id = $2",
    )
    .bind(community.as_uuid())
    .bind(receipt.sale_id)
    .fetch_all(&mut **tx)
    .await?;
    if existing.iter().any(|op| op == receipt.operation) {
        return Ok(ReceiptOutcome::AlreadyRecorded);
    }
    if receipt.operation == "sell_return" && (!voided || !existing.iter().any(|op| op == "sell")) {
        return Ok(ReceiptOutcome::NotReturnable);
    }
    if receipt.total_minor != total {
        return Ok(ReceiptOutcome::TotalMismatch { expected: total });
    }
    let inserted = sqlx::query(
        "INSERT INTO crm_fiscal_receipts \
             (community_id, sale_id, operation, space_channel_id, total_minor, task_uuid, \
              document_number, document_sign, receipt_number, shift_number, fn_number, \
              registration_number, document_datetime, event_id, recorded_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(community.as_uuid())
    .bind(receipt.sale_id)
    .bind(receipt.operation)
    .bind(receipt.space_channel_id)
    .bind(receipt.total_minor)
    .bind(receipt.task_uuid)
    .bind(receipt.document_number)
    .bind(receipt.document_sign)
    .bind(receipt.receipt_number)
    .bind(receipt.shift_number)
    .bind(receipt.fn_number)
    .bind(receipt.registration_number)
    .bind(receipt.document_datetime)
    .bind(receipt.event_id)
    .bind(receipt.actor)
    .execute(&mut **tx)
    .await;
    match inserted {
        Ok(_) => Ok(ReceiptOutcome::Recorded),
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            Ok(ReceiptOutcome::DocumentTaken)
        }
        Err(error) => Err(error.into()),
    }
}

/// A shift event to record.
#[derive(Debug, Clone)]
pub struct ShiftWrite<'a> {
    /// Space channel.
    pub space_channel_id: Uuid,
    /// `open` or `close`.
    pub action: &'a str,
    /// Fiscal storage number.
    pub fn_number: &'a str,
    /// Shift number.
    pub shift_number: i64,
    /// Fiscal document number of the report.
    pub document_number: i64,
    /// Device date and time.
    pub document_datetime: &'a str,
    /// Event author.
    pub actor: &'a [u8],
}

/// Outcome of recording a shift event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftOutcome {
    /// Recorded.
    Recorded,
    /// The shift was already opened (or closed) before.
    AlreadyRecorded,
}

/// Record a shift opened or closed on a ККМ. A close may arrive for a shift
/// whose open was never recorded (opened before the CRM was used).
pub async fn record_shift(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    shift: &ShiftWrite<'_>,
) -> Result<ShiftOutcome> {
    let sql = if shift.action == "open" {
        "INSERT INTO crm_cash_shifts \
             (community_id, space_channel_id, fn_number, shift_number, open_document_number, \
              opened_datetime, opened_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (community_id, fn_number, shift_number) DO UPDATE \
             SET open_document_number = EXCLUDED.open_document_number, \
                 opened_datetime = EXCLUDED.opened_datetime, \
                 opened_by = EXCLUDED.opened_by, updated_at = now() \
             WHERE crm_cash_shifts.open_document_number IS NULL"
    } else {
        "INSERT INTO crm_cash_shifts \
             (community_id, space_channel_id, fn_number, shift_number, close_document_number, \
              closed_datetime, closed_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (community_id, fn_number, shift_number) DO UPDATE \
             SET close_document_number = EXCLUDED.close_document_number, \
                 closed_datetime = EXCLUDED.closed_datetime, \
                 closed_by = EXCLUDED.closed_by, updated_at = now() \
             WHERE crm_cash_shifts.close_document_number IS NULL"
    };
    let written = sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(shift.space_channel_id)
        .bind(shift.fn_number)
        .bind(shift.shift_number)
        .bind(shift.document_number)
        .bind(shift.document_datetime)
        .bind(shift.actor)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(if written == 1 {
        ShiftOutcome::Recorded
    } else {
        ShiftOutcome::AlreadyRecorded
    })
}

/// A shift with its totals over the receipts printed in it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmShiftRecord {
    /// Fiscal storage number.
    pub fn_number: String,
    /// Shift number.
    pub shift_number: i64,
    /// Device time the shift was opened.
    pub opened_datetime: Option<String>,
    /// Device time it was closed.
    pub closed_datetime: Option<String>,
    /// Whether the shift is closed.
    pub closed: bool,
    /// Sale receipts printed in the shift.
    pub sell_count: i64,
    /// Their total, kopecks.
    pub sell_total_minor: i64,
    /// Return receipts printed in the shift.
    pub return_count: i64,
    /// Their total, kopecks.
    pub return_total_minor: i64,
    /// Cash part of the shift's sales, kopecks (minus returned cash).
    pub cash_minor: i64,
    /// Last change.
    pub updated_at: DateTime<Utc>,
}

/// The space's most recent shifts, newest first.
pub async fn list_shifts(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    limit: i64,
) -> Result<Vec<CrmShiftRecord>> {
    let rows = sqlx::query(
        "SELECT sh.fn_number, sh.shift_number, sh.opened_datetime, sh.closed_datetime, \
                sh.close_document_number IS NOT NULL AS closed, sh.updated_at, \
                count(*) FILTER (WHERE fr.operation = 'sell')::bigint AS sell_count, \
                COALESCE(sum(fr.total_minor) FILTER (WHERE fr.operation = 'sell'), 0)::bigint \
                    AS sell_total, \
                count(*) FILTER (WHERE fr.operation = 'sell_return')::bigint AS return_count, \
                COALESCE(sum(fr.total_minor) FILTER (WHERE fr.operation = 'sell_return'), 0)::bigint \
                    AS return_total, \
                COALESCE(sum(CASE fr.operation WHEN 'sell' THEN sp.amount_minor \
                                               ELSE -sp.amount_minor END), 0)::bigint AS cash \
         FROM crm_cash_shifts sh \
         LEFT JOIN crm_fiscal_receipts fr \
                ON fr.community_id = sh.community_id AND fr.fn_number = sh.fn_number \
               AND fr.shift_number = sh.shift_number \
         LEFT JOIN crm_sale_payments sp \
                ON sp.community_id = fr.community_id AND sp.sale_id = fr.sale_id \
               AND sp.method = 'cash' \
         WHERE sh.community_id = $1 AND sh.space_channel_id = $2 \
         GROUP BY sh.community_id, sh.fn_number, sh.shift_number \
         ORDER BY sh.updated_at DESC LIMIT $3",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(CrmShiftRecord {
                fn_number: row.try_get("fn_number")?,
                shift_number: row.try_get("shift_number")?,
                opened_datetime: row.try_get("opened_datetime")?,
                closed_datetime: row.try_get("closed_datetime")?,
                closed: row.try_get("closed")?,
                sell_count: row.try_get("sell_count")?,
                sell_total_minor: row.try_get("sell_total")?,
                return_count: row.try_get("return_count")?,
                return_total_minor: row.try_get("return_total")?,
                cash_minor: row.try_get("cash")?,
                updated_at: row.try_get("updated_at")?,
            })
        })
        .collect()
}

impl Db {
    /// The space's most recent cash-register shifts.
    #[datastore_span(name = "crm_list_shifts", system = "postgresql")]
    pub async fn crm_list_shifts(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        limit: i64,
    ) -> Result<Vec<CrmShiftRecord>> {
        list_shifts(&self.pool, community, space_channel_id, limit).await
    }
}
