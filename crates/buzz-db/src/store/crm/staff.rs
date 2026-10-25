//! CRM staff and working days (migration 0055, kinds 42131, 42132).
//!
//! Writes run inside the caller's event-write transaction (see the parent
//! module). A staff member's services must be bookable products of the same
//! space; the writer checks them under a share lock so a service cannot be
//! retyped to goods while the link commits.

use buzz_datastore_tracing::datastore_span;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Row as _, Transaction};
use uuid::Uuid;

use super::hex_bytes;
use crate::{error::Result, CommunityId, Db};

/// Most staff members `list_staff` returns; a salon has dozens, not thousands.
pub const MAX_STAFF: i64 = 500;
/// Longest day range `list_staff_days` serves.
pub const MAX_SCHEDULE_RANGE_DAYS: i64 = 93;

/// Validated staff fields for one revision.
#[derive(Debug, Clone, Copy)]
pub struct StaffWrite<'a> {
    /// Staff id (`crm-id` tag).
    pub staff_id: Uuid,
    /// Space channel.
    pub space_channel_id: Uuid,
    /// Trimmed name.
    pub name: &'a str,
    /// Job title.
    pub position: Option<&'a str>,
    /// Work phone.
    pub phone: Option<&'a str>,
    /// De-duplicated service (product) ids.
    pub service_ids: &'a [Uuid],
    /// Pubkey of the member who works as this staff member.
    pub member_pubkey: Option<&'a [u8]>,
    /// Archive the staff member.
    pub archived: bool,
    /// Event producing this revision.
    pub revision_event_id: &'a [u8],
    /// Author of the event (recorded as creator on create).
    pub actor: &'a [u8],
}

/// Outcome of a staff write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaffWriteOutcome {
    /// Inserted or updated.
    Written,
    /// Create for an id that already exists.
    AlreadyExists,
    /// Update for an id that does not exist in this space.
    Missing,
    /// Update whose `expected-revision` is stale.
    RevisionMismatch,
    /// A linked service is not a bookable product of this space.
    UnknownService(Uuid),
}

/// A staff row as served by `GET /crm/staff`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmStaffRecord {
    /// Staff id.
    #[serde(rename = "id")]
    pub staff_id: Uuid,
    /// Name.
    pub name: String,
    /// Job title.
    pub position: Option<String>,
    /// Work phone.
    pub phone: Option<String>,
    /// Services this person performs.
    pub service_ids: Vec<Uuid>,
    /// Pubkey of the linked member, hex.
    #[serde(serialize_with = "optional_hex")]
    pub member: Option<Vec<u8>>,
    /// Whether the staff member is archived.
    pub archived: bool,
    /// Id of the event that produced the current row.
    #[serde(rename = "revision", serialize_with = "hex_bytes")]
    pub revision_event_id: Vec<u8>,
    /// First revision.
    pub created_at: DateTime<Utc>,
    /// Last change.
    pub updated_at: DateTime<Utc>,
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

/// One scheduled day of one staff member.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmStaffDayRecord {
    /// Staff id.
    pub staff_id: Uuid,
    /// Calendar day in the space timezone.
    #[serde(rename = "date")]
    pub day: NaiveDate,
    /// `[{"start": minutes, "end": minutes}, …]`; empty for a day off.
    pub intervals: serde_json::Value,
}

/// One day of a schedule write.
#[derive(Debug, Clone)]
pub struct StaffDayWrite {
    /// Calendar day.
    pub day: NaiveDate,
    /// Intervals as stored (`[{"start", "end"}]` in minutes).
    pub intervals: serde_json::Value,
}

/// Outcome of a schedule write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleWriteOutcome {
    /// Days set.
    Written,
    /// The staff member does not exist in this space.
    UnknownStaff,
    /// The staff member is archived.
    ArchivedStaff,
}

/// Create (no `expected_revision`) or CAS-update a staff member.
pub async fn write_staff(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    write: StaffWrite<'_>,
    expected_revision: Option<&[u8]>,
) -> Result<StaffWriteOutcome> {
    let current = sqlx::query(
        "SELECT space_channel_id, revision_event_id FROM crm_staff \
         WHERE community_id = $1 AND staff_id = $2 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(write.staff_id)
    .fetch_optional(&mut **tx)
    .await?;
    match (expected_revision, current) {
        (None, Some(_)) => return Ok(StaffWriteOutcome::AlreadyExists),
        (Some(_), None) => return Ok(StaffWriteOutcome::Missing),
        (Some(expected), Some(row)) => {
            let space: Uuid = row.try_get("space_channel_id")?;
            if space != write.space_channel_id {
                return Ok(StaffWriteOutcome::Missing);
            }
            let revision: Vec<u8> = row.try_get("revision_event_id")?;
            if revision != expected {
                return Ok(StaffWriteOutcome::RevisionMismatch);
            }
        }
        (None, None) => {}
    }

    if !write.service_ids.is_empty() {
        let known: Vec<Uuid> = sqlx::query_scalar(
            "SELECT product_id FROM crm_products \
             WHERE community_id = $1 AND space_channel_id = $2 AND product_id = ANY($3) \
               AND payment_object IN ('service', 'job') \
             FOR SHARE",
        )
        .bind(community.as_uuid())
        .bind(write.space_channel_id)
        .bind(write.service_ids)
        .fetch_all(&mut **tx)
        .await?;
        if let Some(unknown) = write.service_ids.iter().find(|id| !known.contains(id)) {
            return Ok(StaffWriteOutcome::UnknownService(*unknown));
        }
    }

    let create = expected_revision.is_none();
    let query = sqlx::query(if create {
        "INSERT INTO crm_staff \
             (community_id, staff_id, space_channel_id, name, position, phone, service_ids, \
              member_pubkey, archived_at, revision_event_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, CASE WHEN $9 THEN now() END, $10, $11) \
         ON CONFLICT (community_id, staff_id) DO NOTHING"
    } else {
        "UPDATE crm_staff SET \
             name = $4, position = $5, phone = $6, service_ids = $7, member_pubkey = $8, \
             archived_at = CASE WHEN $9 THEN COALESCE(archived_at, now()) END, \
             revision_event_id = $10, updated_at = now() \
         WHERE community_id = $1 AND staff_id = $2 AND space_channel_id = $3"
    })
    .bind(community.as_uuid())
    .bind(write.staff_id)
    .bind(write.space_channel_id)
    .bind(write.name)
    .bind(write.position)
    .bind(write.phone)
    .bind(write.service_ids)
    .bind(write.member_pubkey)
    .bind(write.archived)
    .bind(write.revision_event_id);
    let query = if create {
        query.bind(write.actor)
    } else {
        query
    };
    let written = query.execute(&mut **tx).await?;
    if written.rows_affected() == 1 {
        Ok(StaffWriteOutcome::Written)
    } else {
        // Only reachable for a create racing another create of the same id.
        Ok(StaffWriteOutcome::AlreadyExists)
    }
}

/// Set the listed days of one staff member; other days keep their hours.
pub async fn write_staff_days(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    staff_id: Uuid,
    days: &[StaffDayWrite],
    revision_event_id: &[u8],
) -> Result<ScheduleWriteOutcome> {
    // Share lock: the staff member cannot be archived while the days commit.
    let archived: Option<bool> = sqlx::query_scalar(
        "SELECT archived_at IS NOT NULL FROM crm_staff \
         WHERE community_id = $1 AND space_channel_id = $2 AND staff_id = $3 FOR SHARE",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(staff_id)
    .fetch_optional(&mut **tx)
    .await?;
    match archived {
        None => return Ok(ScheduleWriteOutcome::UnknownStaff),
        Some(true) => return Ok(ScheduleWriteOutcome::ArchivedStaff),
        Some(false) => {}
    }
    let dates: Vec<NaiveDate> = days.iter().map(|day| day.day).collect();
    let intervals: Vec<serde_json::Value> = days.iter().map(|day| day.intervals.clone()).collect();
    sqlx::query(
        "INSERT INTO crm_staff_days \
             (community_id, staff_id, space_channel_id, day, intervals, revision_event_id) \
         SELECT $1, $2, $3, d.day, d.intervals, $6 \
         FROM unnest($4::date[], $5::jsonb[]) AS d(day, intervals) \
         ON CONFLICT (community_id, staff_id, day) DO UPDATE SET \
             intervals = EXCLUDED.intervals, revision_event_id = EXCLUDED.revision_event_id, \
             updated_at = now()",
    )
    .bind(community.as_uuid())
    .bind(staff_id)
    .bind(space_channel_id)
    .bind(&dates)
    .bind(&intervals)
    .bind(revision_event_id)
    .execute(&mut **tx)
    .await?;
    Ok(ScheduleWriteOutcome::Written)
}

macro_rules! staff_columns {
    () => {
        "staff_id, name, position, phone, service_ids, member_pubkey, \
         archived_at IS NOT NULL AS archived, revision_event_id, created_at, updated_at"
    };
}

/// The space's staff in `(lower(name), staff_id)` order.
pub async fn list_staff(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    include_archived: bool,
) -> Result<Vec<CrmStaffRecord>> {
    let sql = concat!(
        "SELECT ",
        staff_columns!(),
        " FROM crm_staff \
         WHERE community_id = $1 AND space_channel_id = $2 AND ($3 OR archived_at IS NULL) \
         ORDER BY lower(name), staff_id LIMIT $4"
    );
    sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(space_channel_id)
        .bind(include_archived)
        .bind(MAX_STAFF)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(staff_from_row)
        .collect()
}

/// One staff member of the space, archived or not.
pub async fn get_staff(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    staff_id: Uuid,
) -> Result<Option<CrmStaffRecord>> {
    let sql = concat!(
        "SELECT ",
        staff_columns!(),
        " FROM crm_staff WHERE community_id = $1 AND space_channel_id = $2 AND staff_id = $3"
    );
    sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(space_channel_id)
        .bind(staff_id)
        .fetch_optional(pool)
        .await?
        .map(staff_from_row)
        .transpose()
}

/// Scheduled days `from..=to` of every staff member of the space.
pub async fn list_staff_days(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<CrmStaffDayRecord>> {
    let rows = sqlx::query(
        "SELECT staff_id, day, intervals FROM crm_staff_days \
         WHERE community_id = $1 AND space_channel_id = $2 AND day BETWEEN $3 AND $4 \
         ORDER BY day, staff_id",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(CrmStaffDayRecord {
                staff_id: row.try_get("staff_id")?,
                day: row.try_get("day")?,
                intervals: row.try_get("intervals")?,
            })
        })
        .collect()
}

fn staff_from_row(row: sqlx::postgres::PgRow) -> Result<CrmStaffRecord> {
    Ok(CrmStaffRecord {
        staff_id: row.try_get("staff_id")?,
        name: row.try_get("name")?,
        position: row.try_get("position")?,
        phone: row.try_get("phone")?,
        service_ids: row.try_get("service_ids")?,
        member: row.try_get("member_pubkey")?,
        archived: row.try_get("archived")?,
        revision_event_id: row.try_get("revision_event_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

impl Db {
    /// The space's staff.
    #[datastore_span(name = "crm_list_staff", system = "postgresql")]
    pub async fn crm_list_staff(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        include_archived: bool,
    ) -> Result<Vec<CrmStaffRecord>> {
        list_staff(&self.pool, community, space_channel_id, include_archived).await
    }

    /// One staff member.
    #[datastore_span(name = "crm_get_staff", system = "postgresql")]
    pub async fn crm_get_staff(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        staff_id: Uuid,
    ) -> Result<Option<CrmStaffRecord>> {
        get_staff(&self.pool, community, space_channel_id, staff_id).await
    }

    /// Scheduled days of the space's staff.
    #[datastore_span(name = "crm_list_staff_days", system = "postgresql")]
    pub async fn crm_list_staff_days(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<CrmStaffDayRecord>> {
        list_staff_days(&self.pool, community, space_channel_id, from, to).await
    }
}

#[cfg(test)]
mod postgres_tests;
