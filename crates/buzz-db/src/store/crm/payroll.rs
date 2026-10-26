//! CRM payroll (migration 0066, kinds 42160–42162): staff pay rates, payouts
//! and the per-period figures accruals are computed from.
//!
//! Writes run inside the caller's event-write transaction (see the parent
//! module) and lock the staff row so a rate or payout cannot land on a staff
//! member of another space.

use buzz_datastore_tracing::datastore_span;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Row as _, Transaction};
use uuid::Uuid;

use super::hex_bytes;
use crate::{error::Result, CommunityId, Db};

/// Most payouts one payroll read lists.
pub const MAX_PAYOUTS: i64 = 1000;

/// Validated rates of one staff member.
#[derive(Debug, Clone, Copy)]
pub struct PayRatesWrite<'a> {
    /// Space channel.
    pub space_channel_id: Uuid,
    /// Staff member.
    pub staff_id: Uuid,
    /// Share of performed services, basis points.
    pub percent_bp: i32,
    /// Per performed visit, kopecks.
    pub per_visit_minor: i64,
    /// Per scheduled working day, kopecks.
    pub shift_minor: i64,
    /// Event producing this revision.
    pub revision_event_id: &'a [u8],
}

/// A validated payout.
#[derive(Debug, Clone, Copy)]
pub struct PayoutWrite<'a> {
    /// Payout id (`crm-id` tag).
    pub payout_id: Uuid,
    /// Space channel.
    pub space_channel_id: Uuid,
    /// Staff member paid.
    pub staff_id: Uuid,
    /// Payout date in the space timezone.
    pub day: NaiveDate,
    /// Amount, kopecks.
    pub amount_minor: i64,
    /// Note.
    pub note: Option<&'a str>,
    /// Event recording the payout.
    pub event_id: &'a [u8],
    /// Author of the event.
    pub actor: &'a [u8],
}

/// Outcome of a rates or payout write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayrollWriteOutcome {
    /// Written.
    Written,
    /// The staff member is not part of this space.
    UnknownStaff,
    /// A payout with this id already exists.
    AlreadyExists,
}

/// Outcome of a payout cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayoutVoidOutcome {
    /// Cancelled.
    Voided,
    /// No such payout in this space.
    Missing,
    /// Cancelled before.
    AlreadyVoided,
}

/// One staff member's payroll figures for a period, before accrual.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StaffPayrollRecord {
    /// Staff id.
    pub staff_id: Uuid,
    /// Current name.
    pub name: String,
    /// Whether the staff member is archived.
    pub archived: bool,
    /// Current share of services, basis points.
    pub percent_bp: i32,
    /// Current amount per visit, kopecks.
    pub per_visit_minor: i64,
    /// Current amount per working day, kopecks.
    pub shift_minor: i64,
    /// Revision of the rates, hex; `None` until rates are set.
    #[serde(serialize_with = "optional_hex")]
    pub rates_revision: Option<Vec<u8>>,
    /// Visits done in the period.
    pub visits: i64,
    /// Booked prices of those visits, kopecks.
    pub revenue_minor: i64,
    /// Scheduled working days in the period up to today.
    pub shifts: i64,
    /// Payouts in the period, kopecks.
    pub paid_minor: i64,
}

fn optional_hex<S: serde::Serializer>(
    value: &Option<Vec<u8>>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    match value {
        Some(bytes) => hex_bytes(bytes, serializer),
        None => serializer.serialize_none(),
    }
}

/// A payout as served by `GET /crm/payroll`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PayoutRecord {
    /// Payout id.
    #[serde(rename = "id")]
    pub payout_id: Uuid,
    /// Staff member paid.
    pub staff_id: Uuid,
    /// Payout date.
    #[serde(rename = "date")]
    pub day: NaiveDate,
    /// Amount, kopecks.
    pub amount_minor: i64,
    /// Note.
    pub note: Option<String>,
    /// Whether the payout was cancelled.
    pub voided: bool,
    /// When it was recorded.
    pub created_at: DateTime<Utc>,
}

/// Payroll of a space for one period.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PayrollRecord {
    /// Active staff, and archived staff with work or payouts in the period.
    pub staff: Vec<StaffPayrollRecord>,
    /// Payouts in the period, newest first, cancelled ones included.
    pub payouts: Vec<PayoutRecord>,
}

/// Lock the staff row and check it belongs to the space.
async fn staff_in_space(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    staff_id: Uuid,
) -> Result<bool> {
    let found: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM crm_staff \
         WHERE community_id = $1 AND space_channel_id = $2 AND staff_id = $3 FOR SHARE",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(staff_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(found.is_some())
}

/// Replace the rates of one staff member.
pub async fn write_pay_rates(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    write: PayRatesWrite<'_>,
) -> Result<PayrollWriteOutcome> {
    if !staff_in_space(tx, community, write.space_channel_id, write.staff_id).await? {
        return Ok(PayrollWriteOutcome::UnknownStaff);
    }
    sqlx::query(
        "INSERT INTO crm_staff_pay \
             (community_id, staff_id, space_channel_id, percent_bp, per_visit_minor, \
              shift_minor, revision_event_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (community_id, staff_id) DO UPDATE SET \
             percent_bp = EXCLUDED.percent_bp, per_visit_minor = EXCLUDED.per_visit_minor, \
             shift_minor = EXCLUDED.shift_minor, \
             revision_event_id = EXCLUDED.revision_event_id, updated_at = now()",
    )
    .bind(community.as_uuid())
    .bind(write.staff_id)
    .bind(write.space_channel_id)
    .bind(write.percent_bp)
    .bind(write.per_visit_minor)
    .bind(write.shift_minor)
    .bind(write.revision_event_id)
    .execute(&mut **tx)
    .await?;
    Ok(PayrollWriteOutcome::Written)
}

/// Record a payout once per id.
pub async fn record_payout(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    write: PayoutWrite<'_>,
) -> Result<PayrollWriteOutcome> {
    if !staff_in_space(tx, community, write.space_channel_id, write.staff_id).await? {
        return Ok(PayrollWriteOutcome::UnknownStaff);
    }
    let inserted = sqlx::query(
        "INSERT INTO crm_staff_payouts \
             (community_id, payout_id, space_channel_id, staff_id, day, amount_minor, note, \
              event_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         ON CONFLICT (community_id, payout_id) DO NOTHING",
    )
    .bind(community.as_uuid())
    .bind(write.payout_id)
    .bind(write.space_channel_id)
    .bind(write.staff_id)
    .bind(write.day)
    .bind(write.amount_minor)
    .bind(write.note)
    .bind(write.event_id)
    .bind(write.actor)
    .execute(&mut **tx)
    .await?;
    Ok(if inserted.rows_affected() == 1 {
        PayrollWriteOutcome::Written
    } else {
        PayrollWriteOutcome::AlreadyExists
    })
}

/// Cancel a payout of the space.
pub async fn void_payout(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    payout_id: Uuid,
    event_id: &[u8],
) -> Result<PayoutVoidOutcome> {
    let voided: Option<bool> = sqlx::query_scalar(
        "SELECT voided_at IS NOT NULL FROM crm_staff_payouts \
         WHERE community_id = $1 AND space_channel_id = $2 AND payout_id = $3 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(payout_id)
    .fetch_optional(&mut **tx)
    .await?;
    match voided {
        None => return Ok(PayoutVoidOutcome::Missing),
        Some(true) => return Ok(PayoutVoidOutcome::AlreadyVoided),
        Some(false) => {}
    }
    sqlx::query(
        "UPDATE crm_staff_payouts SET voided_at = now(), void_event_id = $3 \
         WHERE community_id = $1 AND payout_id = $2",
    )
    .bind(community.as_uuid())
    .bind(payout_id)
    .bind(event_id)
    .execute(&mut **tx)
    .await?;
    Ok(PayoutVoidOutcome::Voided)
}

/// Payroll figures of a space for `from..=to`; working days count only up to
/// `today`, so a month in progress does not pay for days not yet worked.
pub async fn payroll(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    from: NaiveDate,
    to: NaiveDate,
    today: NaiveDate,
) -> Result<PayrollRecord> {
    let rows = sqlx::query(
        "WITH done AS ( \
             SELECT a.staff_id, count(*) AS visits, \
                    COALESCE(sum(a.total_minor), 0)::BIGINT AS revenue_minor \
             FROM crm_appointments a \
             WHERE a.community_id = $1 AND a.space_channel_id = $2 \
               AND a.day BETWEEN $3 AND $4 AND a.status = 'done' \
             GROUP BY a.staff_id \
         ), shifts AS ( \
             SELECT d.staff_id, count(*) AS shifts FROM crm_staff_days d \
             WHERE d.community_id = $1 AND d.space_channel_id = $2 \
               AND d.day BETWEEN $3 AND LEAST($4, $5) AND jsonb_array_length(d.intervals) > 0 \
             GROUP BY d.staff_id \
         ), paid AS ( \
             SELECT p.staff_id, sum(p.amount_minor)::BIGINT AS paid_minor \
             FROM crm_staff_payouts p \
             WHERE p.community_id = $1 AND p.space_channel_id = $2 \
               AND p.day BETWEEN $3 AND $4 AND p.voided_at IS NULL \
             GROUP BY p.staff_id \
         ) \
         SELECT s.staff_id, s.name, s.archived_at IS NOT NULL AS archived, \
                COALESCE(r.percent_bp, 0) AS percent_bp, \
                COALESCE(r.per_visit_minor, 0) AS per_visit_minor, \
                COALESCE(r.shift_minor, 0) AS shift_minor, \
                r.revision_event_id AS rates_revision, \
                COALESCE(d.visits, 0) AS visits, COALESCE(d.revenue_minor, 0) AS revenue_minor, \
                COALESCE(w.shifts, 0) AS shifts, COALESCE(p.paid_minor, 0) AS paid_minor \
         FROM crm_staff s \
         LEFT JOIN crm_staff_pay r ON r.community_id = s.community_id AND r.staff_id = s.staff_id \
         LEFT JOIN done d ON d.staff_id = s.staff_id \
         LEFT JOIN shifts w ON w.staff_id = s.staff_id \
         LEFT JOIN paid p ON p.staff_id = s.staff_id \
         WHERE s.community_id = $1 AND s.space_channel_id = $2 \
           AND (s.archived_at IS NULL OR d.staff_id IS NOT NULL OR w.staff_id IS NOT NULL \
                OR p.staff_id IS NOT NULL) \
         ORDER BY lower(s.name), s.staff_id",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(from)
    .bind(to)
    .bind(today)
    .fetch_all(pool)
    .await?;
    let staff = rows
        .iter()
        .map(|row| {
            Ok(StaffPayrollRecord {
                staff_id: row.try_get("staff_id")?,
                name: row.try_get("name")?,
                archived: row.try_get("archived")?,
                percent_bp: row.try_get("percent_bp")?,
                per_visit_minor: row.try_get("per_visit_minor")?,
                shift_minor: row.try_get("shift_minor")?,
                rates_revision: row.try_get("rates_revision")?,
                visits: row.try_get("visits")?,
                revenue_minor: row.try_get("revenue_minor")?,
                shifts: row.try_get("shifts")?,
                paid_minor: row.try_get("paid_minor")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let payouts = sqlx::query(
        "SELECT payout_id, staff_id, day, amount_minor, note, voided_at IS NOT NULL AS voided, \
                created_at \
         FROM crm_staff_payouts \
         WHERE community_id = $1 AND space_channel_id = $2 AND day BETWEEN $3 AND $4 \
         ORDER BY day DESC, created_at DESC, payout_id LIMIT $5",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(from)
    .bind(to)
    .bind(MAX_PAYOUTS)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|row| {
        Ok(PayoutRecord {
            payout_id: row.try_get("payout_id")?,
            staff_id: row.try_get("staff_id")?,
            day: row.try_get("day")?,
            amount_minor: row.try_get("amount_minor")?,
            note: row.try_get("note")?,
            voided: row.try_get("voided")?,
            created_at: row.try_get("created_at")?,
        })
    })
    .collect::<Result<Vec<_>>>()?;
    Ok(PayrollRecord { staff, payouts })
}

impl Db {
    /// Payroll figures of a CRM space for one period.
    #[datastore_span(name = "crm_payroll", system = "postgresql")]
    pub async fn crm_payroll(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        from: NaiveDate,
        to: NaiveDate,
        today: NaiveDate,
    ) -> Result<PayrollRecord> {
        payroll(&self.pool, community, space_channel_id, from, to, today).await
    }
}
