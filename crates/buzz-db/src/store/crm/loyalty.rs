//! CRM loyalty (migration 0067, kinds 42170, 42171): the bonus programme of a
//! space and each customer's bonus ledger.
//!
//! Writes run inside the caller's event-write transaction under the
//! exclusive space lock (sales and adjustments take it), so a balance read
//! here cannot change before the move that depends on it commits.

use buzz_datastore_tracing::datastore_span;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Row as _, Transaction};
use uuid::Uuid;

use crate::{error::Result, CommunityId, Db};

/// Most moves one customer history read returns.
pub const MAX_BONUS_MOVES: i64 = 200;

/// A space's loyalty programme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LoyaltySettingsRecord {
    /// Whether sales earn and spend bonuses.
    pub enabled: bool,
    /// Bonuses earned, basis points of the amount paid.
    pub cashback_bp: i32,
    /// Most of a sale payable with bonuses, basis points.
    pub max_redeem_bp: i32,
}

/// Where a bonus move comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BonusSource {
    /// Earned on a sale.
    Cashback,
    /// Spent on a sale.
    Redeem,
    /// Manual adjustment.
    Manual,
}

impl BonusSource {
    const fn as_str(self) -> &'static str {
        match self {
            BonusSource::Cashback => "cashback",
            BonusSource::Redeem => "redeem",
            BonusSource::Manual => "manual",
        }
    }
}

/// One bonus move to append.
#[derive(Debug, Clone, Copy)]
pub struct BonusMoveInsert<'a> {
    /// Space channel.
    pub space_channel_id: Uuid,
    /// Customer.
    pub customer_id: Uuid,
    /// Origin.
    pub source: BonusSource,
    /// Kopecks added (positive) or taken (negative).
    pub delta_minor: i64,
    /// Sale behind a cashback or redeem move.
    pub sale_id: Option<Uuid>,
    /// `crm-id` of a manual adjustment.
    pub adjustment_id: Option<Uuid>,
    /// Reason of a manual adjustment.
    pub reason: Option<&'a str>,
    /// Event producing the move.
    pub event_id: &'a [u8],
    /// Event author.
    pub actor: &'a [u8],
}

/// A bonus move as served by `GET /crm/loyalty`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BonusMoveRecord {
    /// Ledger position.
    #[serde(rename = "id")]
    pub move_id: i64,
    /// `cashback`, `redeem`, `manual` or `void`.
    pub source: String,
    /// Kopecks added or taken.
    pub delta_minor: i64,
    /// Sale behind the move.
    pub sale_id: Option<Uuid>,
    /// Reason of a manual adjustment.
    pub reason: Option<String>,
    /// When it was recorded.
    pub created_at: DateTime<Utc>,
}

/// A customer's bonuses.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CustomerBonusesRecord {
    /// Current balance, kopecks (negative after voiding spent cashback).
    pub balance_minor: i64,
    /// Latest moves first.
    pub moves: Vec<BonusMoveRecord>,
}

/// Replace the space's loyalty programme.
pub async fn write_loyalty_settings(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    settings: LoyaltySettingsRecord,
    revision_event_id: &[u8],
) -> Result<()> {
    sqlx::query(
        "INSERT INTO crm_loyalty_settings \
             (community_id, space_channel_id, enabled, cashback_bp, max_redeem_bp, \
              revision_event_id) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (community_id, space_channel_id) DO UPDATE SET \
             enabled = EXCLUDED.enabled, cashback_bp = EXCLUDED.cashback_bp, \
             max_redeem_bp = EXCLUDED.max_redeem_bp, \
             revision_event_id = EXCLUDED.revision_event_id, updated_at = now()",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(settings.enabled)
    .bind(settings.cashback_bp)
    .bind(settings.max_redeem_bp)
    .bind(revision_event_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The space's programme inside a write; `None` until one is set.
pub async fn loyalty_settings_for_write(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
) -> Result<Option<LoyaltySettingsRecord>> {
    let row = sqlx::query(
        "SELECT enabled, cashback_bp, max_redeem_bp FROM crm_loyalty_settings \
         WHERE community_id = $1 AND space_channel_id = $2",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.map(|row| settings_from_row(&row)).transpose()
}

/// A customer's balance inside a write; `None` when the customer is not a
/// party of the space.
pub async fn balance_for_write(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    customer_id: Uuid,
) -> Result<Option<i64>> {
    let known: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM crm_parties \
         WHERE community_id = $1 AND space_channel_id = $2 AND party_id = $3",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(customer_id)
    .fetch_optional(&mut **tx)
    .await?;
    if known.is_none() {
        return Ok(None);
    }
    let balance: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(delta_minor), 0)::BIGINT FROM crm_bonus_moves \
         WHERE community_id = $1 AND customer_id = $2",
    )
    .bind(community.as_uuid())
    .bind(customer_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(Some(balance))
}

/// Append a move; `false` when a manual adjustment id was recorded before.
pub async fn insert_bonus_move(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    m: BonusMoveInsert<'_>,
) -> Result<bool> {
    let inserted = sqlx::query(
        "INSERT INTO crm_bonus_moves \
             (community_id, space_channel_id, customer_id, source, delta_minor, sale_id, \
              adjustment_id, reason, event_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         ON CONFLICT (community_id, adjustment_id) WHERE adjustment_id IS NOT NULL DO NOTHING",
    )
    .bind(community.as_uuid())
    .bind(m.space_channel_id)
    .bind(m.customer_id)
    .bind(m.source.as_str())
    .bind(m.delta_minor)
    .bind(m.sale_id)
    .bind(m.adjustment_id)
    .bind(m.reason)
    .bind(m.event_id)
    .bind(m.actor)
    .execute(&mut **tx)
    .await?;
    Ok(inserted.rows_affected() == 1)
}

/// Append the negation of a sale's earned and spent bonuses.
pub async fn revert_sale_bonuses(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    sale_id: Uuid,
    event_id: &[u8],
    actor: &[u8],
) -> Result<()> {
    sqlx::query(
        "INSERT INTO crm_bonus_moves \
             (community_id, space_channel_id, customer_id, source, delta_minor, sale_id, \
              event_id, created_by) \
         SELECT community_id, space_channel_id, customer_id, 'void', -delta_minor, sale_id, \
                $3, $4 \
         FROM crm_bonus_moves \
         WHERE community_id = $1 AND sale_id = $2 AND source IN ('cashback', 'redeem') \
         ORDER BY move_id",
    )
    .bind(community.as_uuid())
    .bind(sale_id)
    .bind(event_id)
    .bind(actor)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn settings_from_row(row: &sqlx::postgres::PgRow) -> Result<LoyaltySettingsRecord> {
    Ok(LoyaltySettingsRecord {
        enabled: row.try_get("enabled")?,
        cashback_bp: row.try_get("cashback_bp")?,
        max_redeem_bp: row.try_get("max_redeem_bp")?,
    })
}

/// The space's programme; `None` until one is set.
pub async fn loyalty_settings(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
) -> Result<Option<LoyaltySettingsRecord>> {
    let row = sqlx::query(
        "SELECT enabled, cashback_bp, max_redeem_bp FROM crm_loyalty_settings \
         WHERE community_id = $1 AND space_channel_id = $2",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .fetch_optional(pool)
    .await?;
    row.map(|row| settings_from_row(&row)).transpose()
}

/// A customer's balance and latest moves.
pub async fn customer_bonuses(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    customer_id: Uuid,
) -> Result<CustomerBonusesRecord> {
    let balance: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(delta_minor), 0)::BIGINT FROM crm_bonus_moves \
         WHERE community_id = $1 AND space_channel_id = $2 AND customer_id = $3",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(customer_id)
    .fetch_one(pool)
    .await?;
    let moves = sqlx::query(
        "SELECT move_id, source, delta_minor, sale_id, reason, created_at \
         FROM crm_bonus_moves \
         WHERE community_id = $1 AND space_channel_id = $2 AND customer_id = $3 \
         ORDER BY move_id DESC LIMIT $4",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(customer_id)
    .bind(MAX_BONUS_MOVES)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|row| {
        Ok(BonusMoveRecord {
            move_id: row.try_get("move_id")?,
            source: row.try_get("source")?,
            delta_minor: row.try_get("delta_minor")?,
            sale_id: row.try_get("sale_id")?,
            reason: row.try_get("reason")?,
            created_at: row.try_get("created_at")?,
        })
    })
    .collect::<Result<Vec<_>>>()?;
    Ok(CustomerBonusesRecord {
        balance_minor: balance,
        moves,
    })
}

impl Db {
    /// The space's loyalty programme.
    #[datastore_span(name = "crm_loyalty_settings", system = "postgresql")]
    pub async fn crm_loyalty_settings(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
    ) -> Result<Option<LoyaltySettingsRecord>> {
        loyalty_settings(&self.pool, community, space_channel_id).await
    }

    /// A customer's bonus balance and history.
    #[datastore_span(name = "crm_customer_bonuses", system = "postgresql")]
    pub async fn crm_customer_bonuses(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        customer_id: Uuid,
    ) -> Result<CustomerBonusesRecord> {
        customer_bonuses(&self.pool, community, space_channel_id, customer_id).await
    }
}
