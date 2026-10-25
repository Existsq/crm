//! CRM online booking (migrations 0061–0062, kinds 42134–42135).
//!
//! The page settings are a projection of kind 42135 written inside the
//! caller's event-write transaction. Phone codes are relay-owned rows written
//! by the public booking API outside any event: they are the per-phone and
//! per-community send counters and the one-time proof that a client owns the
//! phone they book with.

use buzz_datastore_tracing::datastore_span;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row as _, Transaction};
use uuid::Uuid;

use super::hex_bytes;
use crate::{error::Result, CommunityId, Db};

/// Current kind 42135 row of a space.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OnlineBookingRecord {
    /// Normalized kind 42135 payload.
    pub settings: Value,
    /// Whether the page accepts bookings.
    #[serde(skip)]
    pub enabled: bool,
    /// Current revision event id.
    #[serde(rename = "revision", serialize_with = "hex_bytes")]
    pub revision_event_id: Vec<u8>,
    /// Latest revision time.
    pub updated_at: DateTime<Utc>,
}

/// Outcome of a settings write; anything but `Written` rolls back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnlineBookingWriteOutcome {
    /// Inserted or updated.
    Written,
    /// A create found settings already saved.
    AlreadyExists,
    /// An update found no settings.
    Missing,
    /// The settings changed since `expected_revision`.
    RevisionMismatch,
}

/// A service the public page may offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicService {
    /// Product id.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Catalog category, for grouping.
    pub category: Option<String>,
    /// Duration, minutes.
    pub duration_min: i32,
    /// Price, kopecks.
    pub price_minor: i64,
}

/// A staff member the public page may offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicStaff {
    /// Staff id.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Job title.
    pub position: Option<String>,
    /// Services they perform.
    pub service_ids: Vec<Uuid>,
}

/// A phone code to store.
#[derive(Debug, Clone, Copy)]
pub struct CodeIssue<'a> {
    /// Id the client books with.
    pub verification_id: Uuid,
    /// Phone as the client typed it.
    pub phone: &'a str,
    /// Normalized digits of the phone.
    pub phone_key: &'a str,
    /// HMAC of the code.
    pub code_hash: &'a [u8],
    /// Lifetime of the code, seconds.
    pub ttl_secs: i64,
}

/// Send limits checked before a code is stored.
#[derive(Debug, Clone, Copy)]
pub struct CodeLimits {
    /// Seconds before the same phone may get another code.
    pub resend_after_secs: i64,
    /// Codes one phone may get in 24 hours.
    pub per_phone_per_day: i64,
    /// Codes one community may send in an hour.
    pub per_community_per_hour: i64,
}

/// Outcome of asking for a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeIssueOutcome {
    /// Stored; the caller sends it.
    Issued,
    /// The phone got a code moments ago; retry after this many seconds.
    RetryAfter(i64),
    /// The phone used up its daily codes.
    PhoneLimit,
    /// The community used up its hourly codes.
    CommunityLimit,
}

/// Outcome of checking a code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeCheck {
    /// The code matches; the client owns `phone`.
    Verified {
        /// Phone as the client typed it.
        phone: String,
        /// Normalized digits of the phone.
        phone_key: String,
    },
    /// Wrong code; this many attempts remain.
    Wrong {
        /// Attempts left before the code locks.
        attempts_left: i32,
    },
    /// No such verification.
    Unknown,
    /// The code expired.
    Expired,
    /// The code already booked an appointment.
    Used,
    /// Too many wrong attempts.
    Locked,
}

/// The space's online booking settings.
pub async fn get_online_booking(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
) -> Result<Option<OnlineBookingRecord>> {
    sqlx::query(
        "SELECT settings, enabled, revision_event_id, updated_at FROM crm_online_booking \
         WHERE community_id = $1 AND space_channel_id = $2",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .fetch_optional(pool)
    .await?
    .map(|row| {
        Ok(OnlineBookingRecord {
            settings: row.try_get("settings")?,
            enabled: row.try_get("enabled")?,
            revision_event_id: row.try_get("revision_event_id")?,
            updated_at: row.try_get("updated_at")?,
        })
    })
    .transpose()
}

/// Create (no `expected_revision`) or CAS-update the space's settings.
#[allow(clippy::too_many_arguments)]
pub async fn write_online_booking(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    enabled: bool,
    settings: &Value,
    revision_event_id: &[u8],
    actor: &[u8],
    expected_revision: Option<&[u8]>,
) -> Result<OnlineBookingWriteOutcome> {
    use OnlineBookingWriteOutcome as Out;
    let current: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT revision_event_id FROM crm_online_booking \
         WHERE community_id = $1 AND space_channel_id = $2 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .fetch_optional(&mut **tx)
    .await?;
    let written = match (expected_revision, current) {
        (None, Some(_)) => return Ok(Out::AlreadyExists),
        (Some(_), None) => return Ok(Out::Missing),
        (Some(expected), Some(revision)) if revision != expected => {
            return Ok(Out::RevisionMismatch)
        }
        (Some(_), Some(_)) => {
            sqlx::query(
                "UPDATE crm_online_booking SET enabled = $3, settings = $4, \
                     revision_event_id = $5, updated_at = now() \
                 WHERE community_id = $1 AND space_channel_id = $2",
            )
            .bind(community.as_uuid())
            .bind(space_channel_id)
            .bind(enabled)
            .bind(settings)
            .bind(revision_event_id)
            .execute(&mut **tx)
            .await?
        }
        (None, None) => {
            sqlx::query(
                "INSERT INTO crm_online_booking \
                     (community_id, space_channel_id, enabled, settings, revision_event_id, \
                      created_by) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (community_id, space_channel_id) DO NOTHING",
            )
            .bind(community.as_uuid())
            .bind(space_channel_id)
            .bind(enabled)
            .bind(settings)
            .bind(revision_event_id)
            .bind(actor)
            .execute(&mut **tx)
            .await?
        }
    };
    Ok(if written.rows_affected() == 1 {
        Out::Written
    } else {
        Out::AlreadyExists
    })
}

/// Current local date and time in the space timezone; `None` without a space.
pub async fn space_now_local(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
) -> Result<Option<NaiveDateTime>> {
    Ok(sqlx::query_scalar(
        "SELECT (now() AT TIME ZONE (settings->>'timezone'))::timestamp FROM crm_spaces \
         WHERE community_id = $1 AND channel_id = $2",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .fetch_optional(pool)
    .await?)
}

/// Live services with a duration and live staff of the space, by name.
pub async fn public_catalog(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
) -> Result<(Vec<PublicService>, Vec<PublicStaff>)> {
    let services = sqlx::query(
        "SELECT product_id, name, category, duration_min, price_minor FROM crm_products \
         WHERE community_id = $1 AND space_channel_id = $2 AND archived_at IS NULL \
           AND payment_object IN ('service', 'job') AND duration_min IS NOT NULL \
         ORDER BY lower(coalesce(category, '')), lower(name), product_id",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(PublicService {
            id: row.try_get("product_id")?,
            name: row.try_get("name")?,
            category: row.try_get("category")?,
            duration_min: row.try_get("duration_min")?,
            price_minor: row.try_get("price_minor")?,
        })
    })
    .collect::<Result<Vec<_>>>()?;
    let staff = sqlx::query(
        "SELECT staff_id, name, position, service_ids FROM crm_staff \
         WHERE community_id = $1 AND space_channel_id = $2 AND archived_at IS NULL \
         ORDER BY lower(name), staff_id",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(PublicStaff {
            id: row.try_get("staff_id")?,
            name: row.try_get("name")?,
            position: row.try_get("position")?,
            service_ids: row.try_get("service_ids")?,
        })
    })
    .collect::<Result<Vec<_>>>()?;
    Ok((services, staff))
}

/// Store a code unless a send limit is reached. Issuance is serialized per
/// community, and rows older than a day are dropped first so the table stays
/// bounded by the limits.
pub async fn issue_code(
    pool: &PgPool,
    community: CommunityId,
    issue: CodeIssue<'_>,
    limits: CodeLimits,
) -> Result<CodeIssueOutcome> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('crm_booking_codes:' || $1, 0))")
        .bind(community.as_uuid().to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "DELETE FROM crm_booking_codes \
         WHERE community_id = $1 AND created_at < now() - interval '1 day'",
    )
    .bind(community.as_uuid())
    .execute(&mut *tx)
    .await?;
    let sent_this_hour: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM crm_booking_codes \
         WHERE community_id = $1 AND created_at > now() - interval '1 hour'",
    )
    .bind(community.as_uuid())
    .fetch_one(&mut *tx)
    .await?;
    if sent_this_hour >= limits.per_community_per_hour {
        return Ok(CodeIssueOutcome::CommunityLimit);
    }
    let row = sqlx::query(
        "SELECT count(*) AS sent, \
                coalesce(extract(epoch FROM now() - max(created_at)), 1e9)::bigint AS since \
         FROM crm_booking_codes WHERE community_id = $1 AND phone_key = $2",
    )
    .bind(community.as_uuid())
    .bind(issue.phone_key)
    .fetch_one(&mut *tx)
    .await?;
    let sent: i64 = row.try_get("sent")?;
    let since: i64 = row.try_get("since")?;
    if sent >= limits.per_phone_per_day {
        return Ok(CodeIssueOutcome::PhoneLimit);
    }
    if since < limits.resend_after_secs {
        return Ok(CodeIssueOutcome::RetryAfter(
            limits.resend_after_secs - since,
        ));
    }
    sqlx::query(
        "INSERT INTO crm_booking_codes \
             (community_id, verification_id, phone, phone_key, code_hash, expires_at) \
         VALUES ($1, $2, $3, $4, $5, now() + make_interval(secs => $6))",
    )
    .bind(community.as_uuid())
    .bind(issue.verification_id)
    .bind(issue.phone)
    .bind(issue.phone_key)
    .bind(issue.code_hash)
    .bind(issue.ttl_secs as f64)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(CodeIssueOutcome::Issued)
}

/// Forget a stored code whose SMS could not be sent, so it does not count
/// against the client's limits.
pub async fn discard_code(
    pool: &PgPool,
    community: CommunityId,
    verification_id: Uuid,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM crm_booking_codes \
         WHERE community_id = $1 AND verification_id = $2 AND verified_at IS NULL",
    )
    .bind(community.as_uuid())
    .bind(verification_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Check a code and commit the attempt: a wrong code costs one of
/// `max_attempts`, a right one marks the phone verified (until the code
/// expires or books).
pub async fn check_code(
    pool: &PgPool,
    community: CommunityId,
    verification_id: Uuid,
    code_hash: &[u8],
    max_attempts: i32,
) -> Result<CodeCheck> {
    let mut tx = pool.begin().await?;
    let Some(row) = sqlx::query(
        "SELECT phone, phone_key, code_hash, attempts, expires_at <= now() AS expired, \
                used_at IS NOT NULL AS used \
         FROM crm_booking_codes WHERE community_id = $1 AND verification_id = $2 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(verification_id)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(CodeCheck::Unknown);
    };
    if row.try_get::<bool, _>("used")? {
        return Ok(CodeCheck::Used);
    }
    if row.try_get::<bool, _>("expired")? {
        return Ok(CodeCheck::Expired);
    }
    let attempts: i32 = row.try_get("attempts")?;
    if attempts >= max_attempts {
        return Ok(CodeCheck::Locked);
    }
    let stored: Vec<u8> = row.try_get("code_hash")?;
    if !constant_time_eq(&stored, code_hash) {
        sqlx::query(
            "UPDATE crm_booking_codes SET attempts = attempts + 1 \
             WHERE community_id = $1 AND verification_id = $2",
        )
        .bind(community.as_uuid())
        .bind(verification_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(CodeCheck::Wrong {
            attempts_left: max_attempts - attempts - 1,
        });
    }
    sqlx::query(
        "UPDATE crm_booking_codes SET verified_at = coalesce(verified_at, now()) \
         WHERE community_id = $1 AND verification_id = $2",
    )
    .bind(community.as_uuid())
    .bind(verification_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(CodeCheck::Verified {
        phone: row.try_get("phone")?,
        phone_key: row.try_get("phone_key")?,
    })
}

/// Spend a verified code on `appointment_id` inside the booking transaction;
/// `false` when it is no longer usable (a parallel booking spent it, or it
/// expired since the check).
pub async fn consume_code(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    verification_id: Uuid,
    appointment_id: Uuid,
) -> Result<bool> {
    let spent = sqlx::query(
        "UPDATE crm_booking_codes SET used_at = now(), appointment_id = $3 \
         WHERE community_id = $1 AND verification_id = $2 AND verified_at IS NOT NULL \
           AND used_at IS NULL AND expires_at > now()",
    )
    .bind(community.as_uuid())
    .bind(verification_id)
    .bind(appointment_id)
    .execute(&mut **tx)
    .await?;
    Ok(spent.rows_affected() == 1)
}

/// The one live customer card whose phone digits match `phone_key` (or its
/// Russian `8…` spelling); `None` when there is none or more than one. The
/// appointment write re-checks the card inside its transaction.
pub async fn customer_by_phone(
    executor: impl sqlx::PgExecutor<'_>,
    community: CommunityId,
    space_channel_id: Uuid,
    phone_key: &str,
) -> Result<Option<Uuid>> {
    let mut spellings = vec![phone_key.to_owned()];
    if let Some(rest) = phone_key
        .strip_prefix('7')
        .filter(|_| phone_key.len() == 11)
    {
        spellings.push(format!("8{rest}"));
    }
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT party_id FROM crm_parties \
         WHERE community_id = $1 AND space_channel_id = $2 AND archived_at IS NULL \
           AND party_type IN ('customer', 'both') AND phone_digits = ANY($3) \
         LIMIT 2",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(&spellings)
    .fetch_all(executor)
    .await?;
    Ok(match ids.as_slice() {
        [only] => Some(*only),
        _ => None,
    })
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Db {
    /// The space's online booking settings.
    #[datastore_span(name = "crm_get_online_booking", system = "postgresql")]
    pub async fn crm_get_online_booking(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
    ) -> Result<Option<OnlineBookingRecord>> {
        get_online_booking(&self.pool, community, space_channel_id).await
    }

    /// Current local time of the space.
    #[datastore_span(name = "crm_space_now_local", system = "postgresql")]
    pub async fn crm_space_now_local(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
    ) -> Result<Option<NaiveDateTime>> {
        space_now_local(&self.pool, community, space_channel_id).await
    }

    /// Services and staff the public page may offer.
    #[datastore_span(name = "crm_public_catalog", system = "postgresql")]
    pub async fn crm_public_catalog(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
    ) -> Result<(Vec<PublicService>, Vec<PublicStaff>)> {
        public_catalog(&self.pool, community, space_channel_id).await
    }

    /// Store a phone code unless a limit is reached.
    #[datastore_span(name = "crm_issue_booking_code", system = "postgresql")]
    pub async fn crm_issue_booking_code(
        &self,
        community: CommunityId,
        issue: CodeIssue<'_>,
        limits: CodeLimits,
    ) -> Result<CodeIssueOutcome> {
        issue_code(&self.pool, community, issue, limits).await
    }

    /// Forget a code whose SMS failed.
    #[datastore_span(name = "crm_discard_booking_code", system = "postgresql")]
    pub async fn crm_discard_booking_code(
        &self,
        community: CommunityId,
        verification_id: Uuid,
    ) -> Result<()> {
        discard_code(&self.pool, community, verification_id).await
    }

    /// The one live customer card with this phone.
    #[datastore_span(name = "crm_customer_by_phone", system = "postgresql")]
    pub async fn crm_customer_by_phone(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        phone_key: &str,
    ) -> Result<Option<Uuid>> {
        customer_by_phone(&self.pool, community, space_channel_id, phone_key).await
    }

    /// Check a phone code and commit the attempt.
    #[datastore_span(name = "crm_check_booking_code", system = "postgresql")]
    pub async fn crm_check_booking_code(
        &self,
        community: CommunityId,
        verification_id: Uuid,
        code_hash: &[u8],
        max_attempts: i32,
    ) -> Result<CodeCheck> {
        check_code(
            &self.pool,
            community,
            verification_id,
            code_hash,
            max_attempts,
        )
        .await
    }
}

#[cfg(test)]
mod postgres_tests;
