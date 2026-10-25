//! CRM projections: spaces and warehouses (migration 0050); the product
//! catalog lives in [`catalog`] (migration 0051), customers and suppliers in
//! [`parties`] (0052), sale documents in [`sales`] (0053) and the read-only
//! [`analytics`] over them; fiscal receipts and cash shifts in [`fiscal`]
//! (0054); staff and their working days in [`staff`] (0055); appointments
//! in [`appointments`] (0056) with [`visit_analytics`] over them; the stock ledger and service tech cards in
//! [`stock`] (0060).
//!
//! Writes run inside the caller's event-write transaction so the signed CRM
//! event and its projection rows commit or roll back together (ADR-0001).
//! Authorization reads (`channel_role_for_write`, `relay_role_for_write`) are
//! done in that same transaction with row locks, so a member removed a moment
//! ago cannot slip a write through a stale cache.

use buzz_datastore_tracing::datastore_span;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Row as _, Transaction};
use uuid::Uuid;

use crate::{error::Result, CommunityId, Db};

pub mod analytics;
pub mod appointments;
pub mod catalog;
pub mod fiscal;
pub mod parties;
pub mod sales;
pub mod staff;
pub mod stock;
pub mod visit_analytics;

pub use analytics::{AnalyticsPeriod, SalesAnalytics};
pub use appointments::{
    AppointmentPayOutcome, AppointmentQuery, AppointmentServiceWrite, AppointmentWrite,
    AppointmentWriteOutcome, CrmAppointmentRecord, SlotInputs, MAX_APPOINTMENT_RANGE_DAYS,
};
pub use catalog::{
    CrmProductRecord, ProductPage, ProductQuery, ProductWrite, ProductWriteOutcome,
    MAX_PRODUCT_PAGE,
};
pub use fiscal::{CrmShiftRecord, ReceiptOutcome, ReceiptWrite, ShiftOutcome, ShiftWrite};
pub use parties::{
    CrmPartyRecord, PartyPage, PartyQuery, PartyRole, PartyWrite, PartyWriteOutcome, MAX_PARTY_PAGE,
};
pub use sales::{
    CrmSaleRecord, SaleInsert, SaleLineInsert, SalePage, SaleQuery, SaleVoidOutcome, MAX_SALE_PAGE,
};
pub use staff::{
    CrmStaffDayRecord, CrmStaffRecord, ScheduleWriteOutcome, StaffDayWrite, StaffWrite,
    StaffWriteOutcome, MAX_SCHEDULE_RANGE_DAYS,
};
pub use stock::{
    CrmStockDocRecord, CrmStockMoveRecord, CrmStockRecord, CrmTechCardRecord, StockDocPage,
    StockDocQuery, StockDocVoidOutcome, StockList, StockMovePage, StockMoveQuery, StockQuery,
    TechCardWriteOutcome, MAX_STOCK_LIST, MAX_STOCK_PAGE,
};

/// Channel facts needed to decide whether a channel can host a CRM space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrmChannelFacts {
    /// `channel_type` enum value (`stream`, `forum`, `dm`, `workflow`).
    pub channel_type: String,
    /// `channel_visibility` enum value (`open`, `private`).
    pub visibility: String,
    /// Whether the channel is archived.
    pub archived: bool,
    /// Whether the channel carries a TTL.
    pub has_ttl: bool,
}

/// Current CRM space row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmSpaceRecord {
    /// The bound private channel (also the space id).
    pub channel_id: Uuid,
    /// Space settings as stored (validated `buzz_crm::SpaceSettings`).
    pub settings: serde_json::Value,
    /// Id of the event that produced the current settings.
    #[serde(serialize_with = "hex_bytes")]
    pub revision_event_id: Vec<u8>,
    /// Creator pubkey.
    #[serde(serialize_with = "hex_bytes")]
    pub created_by: Vec<u8>,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last settings change.
    pub updated_at: DateTime<Utc>,
}

/// A warehouse row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmWarehouseRecord {
    /// Warehouse id.
    pub warehouse_id: Uuid,
    /// Display name.
    pub name: String,
    /// Whether this is the space default.
    pub is_default: bool,
    /// Whether the warehouse is archived.
    pub archived: bool,
    /// Id of the event that produced the current row.
    #[serde(serialize_with = "hex_bytes")]
    pub revision_event_id: Vec<u8>,
}

fn hex_bytes<S: serde::Serializer>(
    bytes: &[u8],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(&hex::encode(bytes))
}

/// Outcome of creating a space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceCreateOutcome {
    /// Space and default warehouse inserted.
    Created,
    /// The community already has a CRM space.
    CommunityHasSpace,
    /// The default warehouse id is already used in this community.
    WarehouseIdTaken,
}

/// Outcome of a compare-and-swap update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasOutcome {
    /// Row updated.
    Updated,
    /// No row with this id exists.
    Missing,
    /// The row's revision is not the expected one.
    RevisionMismatch,
}

/// Outcome of a warehouse write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarehouseWriteOutcome {
    /// Inserted or updated.
    Written,
    /// Create for an id that already exists.
    AlreadyExists,
    /// Update for an id that does not exist in this space.
    Missing,
    /// Update whose `expected-revision` is stale.
    RevisionMismatch,
    /// The write would leave the space without a default warehouse.
    WouldRemoveDefault,
}

/// Validated warehouse fields.
#[derive(Debug, Clone, Copy)]
pub struct WarehouseWrite<'a> {
    /// Warehouse id (`crm-id` tag).
    pub warehouse_id: Uuid,
    /// Space channel.
    pub space_channel_id: Uuid,
    /// Trimmed display name.
    pub name: &'a str,
    /// Make this the default warehouse.
    pub is_default: bool,
    /// Archive the warehouse.
    pub archived: bool,
    /// Event producing this revision.
    pub revision_event_id: &'a [u8],
}

/// Lock and return the facts of a channel that may host a CRM space.
///
/// `FOR SHARE` blocks a concurrent visibility/TTL edit of the same channel
/// until this transaction commits. Returns `None` for missing or deleted
/// channels.
pub async fn channel_facts_for_write(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    channel_id: Uuid,
) -> Result<Option<CrmChannelFacts>> {
    let row = sqlx::query(
        "SELECT channel_type::text AS channel_type, visibility::text AS visibility, \
                archived_at IS NOT NULL AS archived, ttl_seconds IS NOT NULL AS has_ttl \
         FROM channels \
         WHERE community_id = $1 AND id = $2 AND deleted_at IS NULL \
         FOR SHARE",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.map(|row| {
        Ok(CrmChannelFacts {
            channel_type: row.try_get("channel_type")?,
            visibility: row.try_get("visibility")?,
            archived: row.try_get("archived")?,
            has_ttl: row.try_get("has_ttl")?,
        })
    })
    .transpose()
}

/// The caller's active role in `channel_id`, locked for the transaction.
pub async fn channel_role_for_write(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    channel_id: Uuid,
    pubkey: &[u8],
) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT role::text FROM channel_members \
         WHERE community_id = $1 AND channel_id = $2 AND pubkey = $3 AND removed_at IS NULL \
         FOR SHARE",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .bind(pubkey)
    .fetch_optional(&mut **tx)
    .await?)
}

/// The caller's community (relay-member) role, locked for the transaction.
pub async fn relay_role_for_write(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    pubkey_hex: &str,
) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT role FROM relay_members WHERE community_id = $1 AND pubkey = $2 FOR SHARE",
    )
    .bind(community.as_uuid())
    .bind(pubkey_hex)
    .fetch_optional(&mut **tx)
    .await?)
}

/// Whether PostgreSQL knows the IANA timezone `name`.
pub async fn timezone_exists(tx: &mut Transaction<'_, Postgres>, name: &str) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_timezone_names WHERE name = $1)")
            .bind(name)
            .fetch_one(&mut **tx)
            .await?,
    )
}

/// The space bound to `channel_id`, locked `FOR UPDATE` (`exclusive`: settings
/// and warehouse writes, which must serialize per space) or `FOR SHARE`.
pub async fn space_for_write(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    channel_id: Uuid,
    exclusive: bool,
) -> Result<Option<CrmSpaceRecord>> {
    let sql = if exclusive {
        "SELECT channel_id, settings, revision_event_id, created_by, created_at, updated_at \
         FROM crm_spaces WHERE community_id = $1 AND channel_id = $2 FOR UPDATE"
    } else {
        "SELECT channel_id, settings, revision_event_id, created_by, created_at, updated_at \
         FROM crm_spaces WHERE community_id = $1 AND channel_id = $2 FOR SHARE"
    };
    let row = sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(channel_id)
        .fetch_optional(&mut **tx)
        .await?;
    row.map(space_from_row).transpose()
}

/// Create the space and its default warehouse.
#[allow(clippy::too_many_arguments)]
pub async fn create_space(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    channel_id: Uuid,
    settings: &serde_json::Value,
    revision_event_id: &[u8],
    created_by: &[u8],
    warehouse_id: Uuid,
    warehouse_name: &str,
) -> Result<SpaceCreateOutcome> {
    let inserted = sqlx::query(
        "INSERT INTO crm_spaces (community_id, channel_id, settings, revision_event_id, created_by) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .bind(settings)
    .bind(revision_event_id)
    .bind(created_by)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if inserted == 0 {
        return Ok(SpaceCreateOutcome::CommunityHasSpace);
    }
    let warehouse = sqlx::query(
        "INSERT INTO crm_warehouses \
             (community_id, warehouse_id, space_channel_id, name, is_default, revision_event_id) \
         VALUES ($1, $2, $3, $4, TRUE, $5) ON CONFLICT DO NOTHING",
    )
    .bind(community.as_uuid())
    .bind(warehouse_id)
    .bind(channel_id)
    .bind(warehouse_name)
    .bind(revision_event_id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if warehouse == 0 {
        return Ok(SpaceCreateOutcome::WarehouseIdTaken);
    }
    Ok(SpaceCreateOutcome::Created)
}

/// Replace the settings when the current revision is `expected_revision`.
pub async fn update_space_settings(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    channel_id: Uuid,
    settings: &serde_json::Value,
    expected_revision: &[u8],
    revision_event_id: &[u8],
) -> Result<CasOutcome> {
    let updated = sqlx::query(
        "UPDATE crm_spaces SET settings = $3, revision_event_id = $5, updated_at = now() \
         WHERE community_id = $1 AND channel_id = $2 AND revision_event_id = $4",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .bind(settings)
    .bind(expected_revision)
    .bind(revision_event_id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if updated == 1 {
        return Ok(CasOutcome::Updated);
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM crm_spaces WHERE community_id = $1 AND channel_id = $2)",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(if exists {
        CasOutcome::RevisionMismatch
    } else {
        CasOutcome::Missing
    })
}

/// Create (no `expected_revision`) or CAS-update a warehouse.
///
/// Setting `is_default` moves the default flag here in the same transaction;
/// clearing it on the current default, or archiving the default, is refused
/// with [`WarehouseWriteOutcome::WouldRemoveDefault`].
pub async fn write_warehouse(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    write: WarehouseWrite<'_>,
    expected_revision: Option<&[u8]>,
) -> Result<WarehouseWriteOutcome> {
    let current = sqlx::query(
        "SELECT space_channel_id, is_default, revision_event_id FROM crm_warehouses \
         WHERE community_id = $1 AND warehouse_id = $2 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(write.warehouse_id)
    .fetch_optional(&mut **tx)
    .await?;

    match (expected_revision, current) {
        (None, Some(_)) => return Ok(WarehouseWriteOutcome::AlreadyExists),
        (Some(_), None) => return Ok(WarehouseWriteOutcome::Missing),
        (Some(expected), Some(row)) => {
            let space: Uuid = row.try_get("space_channel_id")?;
            if space != write.space_channel_id {
                return Ok(WarehouseWriteOutcome::Missing);
            }
            let revision: Vec<u8> = row.try_get("revision_event_id")?;
            if revision != expected {
                return Ok(WarehouseWriteOutcome::RevisionMismatch);
            }
            let was_default: bool = row.try_get("is_default")?;
            if was_default && (!write.is_default || write.archived) {
                return Ok(WarehouseWriteOutcome::WouldRemoveDefault);
            }
        }
        (None, None) => {}
    }

    if write.is_default {
        // The demoted warehouse changed too: give it this event's revision so
        // a client holding its old revision cannot silently re-promote it.
        sqlx::query(
            "UPDATE crm_warehouses \
             SET is_default = FALSE, revision_event_id = $4, updated_at = now() \
             WHERE community_id = $1 AND space_channel_id = $2 AND is_default \
               AND warehouse_id <> $3",
        )
        .bind(community.as_uuid())
        .bind(write.space_channel_id)
        .bind(write.warehouse_id)
        .bind(write.revision_event_id)
        .execute(&mut **tx)
        .await?;
    }

    let written = if expected_revision.is_none() {
        sqlx::query(
            "INSERT INTO crm_warehouses \
                 (community_id, warehouse_id, space_channel_id, name, is_default, archived_at, \
                  revision_event_id) \
             VALUES ($1, $2, $3, $4, $5, CASE WHEN $6 THEN now() END, $7) \
             ON CONFLICT (community_id, warehouse_id) DO NOTHING",
        )
    } else {
        sqlx::query(
            "UPDATE crm_warehouses SET \
                 name = $4, \
                 is_default = $5, \
                 archived_at = CASE WHEN $6 THEN COALESCE(archived_at, now()) END, \
                 revision_event_id = $7, \
                 updated_at = now() \
             WHERE community_id = $1 AND warehouse_id = $2 AND space_channel_id = $3",
        )
    }
    .bind(community.as_uuid())
    .bind(write.warehouse_id)
    .bind(write.space_channel_id)
    .bind(write.name)
    .bind(write.is_default)
    .bind(write.archived)
    .bind(write.revision_event_id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(if written == 1 {
        WarehouseWriteOutcome::Written
    } else {
        // Only reachable for a create racing another create of the same id.
        WarehouseWriteOutcome::AlreadyExists
    })
}

/// The community's CRM space, if any.
pub async fn get_space(pool: &PgPool, community: CommunityId) -> Result<Option<CrmSpaceRecord>> {
    let row = sqlx::query(
        "SELECT channel_id, settings, revision_event_id, created_by, created_at, updated_at \
         FROM crm_spaces WHERE community_id = $1",
    )
    .bind(community.as_uuid())
    .fetch_optional(pool)
    .await?;
    row.map(space_from_row).transpose()
}

/// Warehouses of a space, default first, then by name.
pub async fn list_warehouses(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    include_archived: bool,
) -> Result<Vec<CrmWarehouseRecord>> {
    let rows = sqlx::query(
        "SELECT warehouse_id, name, is_default, archived_at IS NOT NULL AS archived, \
                revision_event_id \
         FROM crm_warehouses \
         WHERE community_id = $1 AND space_channel_id = $2 AND ($3 OR archived_at IS NULL) \
         ORDER BY is_default DESC, name, warehouse_id",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(include_archived)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(CrmWarehouseRecord {
                warehouse_id: row.try_get("warehouse_id")?,
                name: row.try_get("name")?,
                is_default: row.try_get("is_default")?,
                archived: row.try_get("archived")?,
                revision_event_id: row.try_get("revision_event_id")?,
            })
        })
        .collect()
}

/// Whether `channel_id` is the community's CRM space channel.
pub async fn is_space_channel(
    pool: &PgPool,
    community: CommunityId,
    channel_id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM crm_spaces WHERE community_id = $1 AND channel_id = $2)",
    )
    .bind(community.as_uuid())
    .bind(channel_id)
    .fetch_one(pool)
    .await?)
}

fn space_from_row(row: sqlx::postgres::PgRow) -> Result<CrmSpaceRecord> {
    Ok(CrmSpaceRecord {
        channel_id: row.try_get("channel_id")?,
        settings: row.try_get("settings")?,
        revision_event_id: row.try_get("revision_event_id")?,
        created_by: row.try_get("created_by")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

impl Db {
    /// The community's CRM space, if any.
    #[datastore_span(name = "crm_get_space", system = "postgresql")]
    pub async fn crm_get_space(&self, community: CommunityId) -> Result<Option<CrmSpaceRecord>> {
        get_space(&self.pool, community).await
    }

    /// Warehouses of a CRM space.
    #[datastore_span(name = "crm_list_warehouses", system = "postgresql")]
    pub async fn crm_list_warehouses(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        include_archived: bool,
    ) -> Result<Vec<CrmWarehouseRecord>> {
        list_warehouses(&self.pool, community, space_channel_id, include_archived).await
    }

    /// Whether `channel_id` is the community's CRM space channel.
    #[datastore_span(name = "crm_is_space_channel", system = "postgresql")]
    pub async fn crm_is_space_channel(
        &self,
        community: CommunityId,
        channel_id: Uuid,
    ) -> Result<bool> {
        is_space_channel(&self.pool, community, channel_id).await
    }
}

#[cfg(test)]
mod postgres_tests;
