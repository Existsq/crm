//! CRM customers and suppliers projection (migration 0052, kind 42103).
//!
//! Writes run inside the caller's event-write transaction (see the parent
//! module). INN uniqueness is enforced by a unique index; the writer checks it
//! first so the rejection can name the duplicate, and maps a unique violation
//! from a concurrent writer to the same outcome.

use buzz_datastore_tracing::datastore_span;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Row as _, Transaction};
use uuid::Uuid;

use super::catalog::like_escape;
use super::hex_bytes;
use crate::{error::Result, CommunityId, Db};

/// Largest page `list_parties` returns.
pub const MAX_PARTY_PAGE: i64 = 200;

/// Validated party fields for one revision.
#[derive(Debug, Clone, Copy)]
pub struct PartyWrite<'a> {
    /// Party id (`crm-id` tag).
    pub party_id: Uuid,
    /// Space channel.
    pub space_channel_id: Uuid,
    /// `customer`, `supplier` or `both`.
    pub party_type: &'a str,
    /// Trimmed name.
    pub name: &'a str,
    /// Phone as typed.
    pub phone: Option<&'a str>,
    /// Digits of the phone.
    pub phone_digits: Option<&'a str>,
    /// Email.
    pub email: Option<&'a str>,
    /// Validated INN.
    pub inn: Option<&'a str>,
    /// Address.
    pub address: Option<&'a str>,
    /// Note.
    pub note: Option<&'a str>,
    /// Archive the party.
    pub archived: bool,
    /// Event producing this revision.
    pub revision_event_id: &'a [u8],
    /// Author of the event (recorded as creator on create).
    pub actor: &'a [u8],
}

/// Outcome of a party write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartyWriteOutcome {
    /// Inserted or updated.
    Written,
    /// Create for an id that already exists.
    AlreadyExists,
    /// Update for an id that does not exist in this space.
    Missing,
    /// Update whose `expected-revision` is stale.
    RevisionMismatch,
    /// Another party in the space has this INN.
    InnTaken(Option<String>),
}

/// Purchase totals of a customer over non-voided sales.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PartyPurchaseStats {
    /// Number of sales.
    pub sales_count: i64,
    /// Sum of sale totals in kopecks.
    pub revenue_minor: i64,
    /// Time of the latest sale.
    pub last_sale_at: Option<DateTime<Utc>>,
}

/// A party row as served by `GET /crm/parties`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmPartyRecord {
    /// Party id.
    #[serde(rename = "id")]
    pub party_id: Uuid,
    /// `customer`, `supplier` or `both`.
    #[serde(rename = "type")]
    pub party_type: String,
    /// Name.
    pub name: String,
    /// Phone.
    pub phone: Option<String>,
    /// Email.
    pub email: Option<String>,
    /// INN.
    pub inn: Option<String>,
    /// Address.
    pub address: Option<String>,
    /// Note.
    pub note: Option<String>,
    /// Whether the party is archived.
    pub archived: bool,
    /// Purchases as a customer.
    pub purchases: PartyPurchaseStats,
    /// Id of the event that produced the current row.
    #[serde(rename = "revision", serialize_with = "hex_bytes")]
    pub revision_event_id: Vec<u8>,
    /// First revision.
    pub created_at: DateTime<Utc>,
    /// Last change.
    pub updated_at: DateTime<Utc>,
}

/// Filters for [`list_parties`].
#[derive(Debug, Clone, Default)]
pub struct PartyQuery {
    /// Name substring, phone digits, INN prefix or email substring.
    pub q: Option<String>,
    /// Only parties that buy (`customer`, `both`) or sell (`supplier`, `both`).
    pub role: Option<PartyRole>,
    /// Include archived parties.
    pub include_archived: bool,
    /// Keyset cursor: the id of the last party of the previous page.
    pub after: Option<Uuid>,
    /// Page size (clamped to 1..=[`MAX_PARTY_PAGE`]).
    pub limit: i64,
}

/// Role filter of [`PartyQuery`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartyRole {
    /// Customers and parties of type `both`.
    Customers,
    /// Suppliers and parties of type `both`.
    Suppliers,
}

/// One page of parties.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PartyPage {
    /// Parties in `(lower(name), id)` order.
    pub parties: Vec<CrmPartyRecord>,
    /// Cursor for the next page, when more rows exist.
    pub next: Option<Uuid>,
}

/// Facts a sale needs about its customer, read under a share lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyForSale {
    /// `customer`, `supplier` or `both`.
    pub party_type: String,
    /// Whether the party is archived.
    pub archived: bool,
}

/// Create (no `expected_revision`) or CAS-update a party.
pub async fn write_party(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    write: PartyWrite<'_>,
    expected_revision: Option<&[u8]>,
) -> Result<PartyWriteOutcome> {
    let current = sqlx::query(
        "SELECT space_channel_id, revision_event_id FROM crm_parties \
         WHERE community_id = $1 AND party_id = $2 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(write.party_id)
    .fetch_optional(&mut **tx)
    .await?;
    match (expected_revision, current) {
        (None, Some(_)) => return Ok(PartyWriteOutcome::AlreadyExists),
        (Some(_), None) => return Ok(PartyWriteOutcome::Missing),
        (Some(expected), Some(row)) => {
            let space: Uuid = row.try_get("space_channel_id")?;
            if space != write.space_channel_id {
                return Ok(PartyWriteOutcome::Missing);
            }
            let revision: Vec<u8> = row.try_get("revision_event_id")?;
            if revision != expected {
                return Ok(PartyWriteOutcome::RevisionMismatch);
            }
        }
        (None, None) => {}
    }

    if let Some(inn) = write.inn {
        let taken: Option<Uuid> = sqlx::query_scalar(
            "SELECT party_id FROM crm_parties \
             WHERE community_id = $1 AND space_channel_id = $2 AND inn = $3 AND party_id <> $4 \
             LIMIT 1",
        )
        .bind(community.as_uuid())
        .bind(write.space_channel_id)
        .bind(inn)
        .bind(write.party_id)
        .fetch_optional(&mut **tx)
        .await?;
        if taken.is_some() {
            return Ok(PartyWriteOutcome::InnTaken(Some(inn.to_owned())));
        }
    }

    let written = if expected_revision.is_none() {
        bind_party_fields(
            sqlx::query(
                "INSERT INTO crm_parties \
                     (community_id, party_id, space_channel_id, party_type, name, phone, \
                      phone_digits, email, inn, address, note, archived_at, revision_event_id, \
                      created_by) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, \
                         CASE WHEN $12 THEN now() END, $13, $14) \
                 ON CONFLICT (community_id, party_id) DO NOTHING",
            ),
            community,
            &write,
        )
        .bind(write.actor)
        .execute(&mut **tx)
        .await
    } else {
        bind_party_fields(
            sqlx::query(
                "UPDATE crm_parties SET \
                     party_type = $4, name = $5, phone = $6, phone_digits = $7, email = $8, \
                     inn = $9, address = $10, note = $11, \
                     archived_at = CASE WHEN $12 THEN COALESCE(archived_at, now()) END, \
                     revision_event_id = $13, updated_at = now() \
                 WHERE community_id = $1 AND party_id = $2 AND space_channel_id = $3",
            ),
            community,
            &write,
        )
        .execute(&mut **tx)
        .await
    };
    match written {
        Ok(result) if result.rows_affected() == 1 => Ok(PartyWriteOutcome::Written),
        // Only reachable for a create racing another create of the same id.
        Ok(_) => Ok(PartyWriteOutcome::AlreadyExists),
        Err(sqlx::Error::Database(db_error)) if db_error.code().as_deref() == Some("23505") => {
            Ok(PartyWriteOutcome::InnTaken(None))
        }
        Err(error) => Err(error.into()),
    }
}

type PgQuery<'q> = sqlx::query::Query<'q, Postgres, sqlx::postgres::PgArguments>;

/// Bind `$1..$13` shared by the party insert and update.
fn bind_party_fields<'q>(
    query: PgQuery<'q>,
    community: CommunityId,
    write: &PartyWrite<'q>,
) -> PgQuery<'q> {
    query
        .bind(community.as_uuid())
        .bind(write.party_id)
        .bind(write.space_channel_id)
        .bind(write.party_type)
        .bind(write.name)
        .bind(write.phone)
        .bind(write.phone_digits)
        .bind(write.email)
        .bind(write.inn)
        .bind(write.address)
        .bind(write.note)
        .bind(write.archived)
        .bind(write.revision_event_id)
}

/// The party a sale names, share-locked so it cannot be archived or retyped
/// while the sale commits.
pub async fn party_for_sale(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    space_channel_id: Uuid,
    party_id: Uuid,
) -> Result<Option<PartyForSale>> {
    let row = sqlx::query(
        "SELECT party_type, archived_at IS NOT NULL AS archived FROM crm_parties \
         WHERE community_id = $1 AND space_channel_id = $2 AND party_id = $3 FOR SHARE",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(party_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.map(|row| {
        Ok(PartyForSale {
            party_type: row.try_get("party_type")?,
            archived: row.try_get("archived")?,
        })
    })
    .transpose()
}

/// Select list shared by the party reads (a macro so every statement stays a
/// `&'static str`). Purchase totals come from the customer index of
/// `crm_sales`, voided sales excluded.
macro_rules! party_columns {
    () => {
        "p.party_id, p.party_type, p.name, p.phone, p.email, p.inn, p.address, p.note, \
         p.archived_at IS NOT NULL AS archived, p.revision_event_id, p.created_at, p.updated_at, \
         s.sales_count, s.revenue_minor, s.last_sale_at"
    };
}

macro_rules! party_stats_join {
    () => {
        " LEFT JOIN LATERAL ( \
             SELECT count(*)::bigint AS sales_count, \
                    COALESCE(sum(total_minor), 0)::bigint AS revenue_minor, \
                    max(sold_at) AS last_sale_at \
             FROM crm_sales sa \
             WHERE sa.community_id = p.community_id AND sa.customer_id = p.party_id \
               AND sa.voided_at IS NULL) s ON TRUE"
    };
}

/// One page of a space's parties ordered by `(lower(name), party_id)`.
pub async fn list_parties(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    query: &PartyQuery,
) -> Result<PartyPage> {
    let limit = query.limit.clamp(1, MAX_PARTY_PAGE);
    let q = query.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let pattern = q.map(like_escape);
    // Digits of the search text match phone numbers only when the text has
    // enough of them to be a phone fragment.
    let digits = q
        .map(|q| q.chars().filter(char::is_ascii_digit).collect::<String>())
        .filter(|d| d.len() >= 3);
    let (customers, suppliers) = match query.role {
        None => (false, false),
        Some(PartyRole::Customers) => (true, false),
        Some(PartyRole::Suppliers) => (false, true),
    };
    let sql = concat!(
        "SELECT ",
        party_columns!(),
        " FROM crm_parties p",
        party_stats_join!(),
        " WHERE p.community_id = $1 AND p.space_channel_id = $2 \
           AND ($3 OR p.archived_at IS NULL) \
           AND (NOT $4 OR p.party_type IN ('customer', 'both')) \
           AND (NOT $5 OR p.party_type IN ('supplier', 'both')) \
           AND ($6::text IS NULL \
                OR p.name ILIKE '%' || $6 || '%' \
                OR p.email ILIKE '%' || $6 || '%' \
                OR p.inn LIKE $6 || '%' \
                OR ($7::text IS NOT NULL AND p.phone_digits LIKE '%' || $7 || '%')) \
           AND ($8::uuid IS NULL OR (lower(p.name), p.party_id) > ( \
                SELECT lower(c.name), c.party_id FROM crm_parties c \
                WHERE c.community_id = $1 AND c.party_id = $8)) \
         ORDER BY lower(p.name), p.party_id \
         LIMIT $9"
    );
    let rows = sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(space_channel_id)
        .bind(query.include_archived)
        .bind(customers)
        .bind(suppliers)
        .bind(pattern)
        .bind(digits)
        .bind(query.after)
        .bind(limit + 1)
        .fetch_all(pool)
        .await?;
    let mut parties = rows
        .into_iter()
        .map(party_from_row)
        .collect::<Result<Vec<_>>>()?;
    let next = if parties.len() as i64 > limit {
        parties.truncate(limit as usize);
        parties.last().map(|p| p.party_id)
    } else {
        None
    };
    Ok(PartyPage { parties, next })
}

/// One party of the space, archived or not.
pub async fn get_party(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    party_id: Uuid,
) -> Result<Option<CrmPartyRecord>> {
    let sql = concat!(
        "SELECT ",
        party_columns!(),
        " FROM crm_parties p",
        party_stats_join!(),
        " WHERE p.community_id = $1 AND p.space_channel_id = $2 AND p.party_id = $3"
    );
    let row = sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(space_channel_id)
        .bind(party_id)
        .fetch_optional(pool)
        .await?;
    row.map(party_from_row).transpose()
}

fn party_from_row(row: sqlx::postgres::PgRow) -> Result<CrmPartyRecord> {
    Ok(CrmPartyRecord {
        party_id: row.try_get("party_id")?,
        party_type: row.try_get("party_type")?,
        name: row.try_get("name")?,
        phone: row.try_get("phone")?,
        email: row.try_get("email")?,
        inn: row.try_get("inn")?,
        address: row.try_get("address")?,
        note: row.try_get("note")?,
        archived: row.try_get("archived")?,
        purchases: PartyPurchaseStats {
            sales_count: row.try_get("sales_count")?,
            revenue_minor: row.try_get("revenue_minor")?,
            last_sale_at: row.try_get("last_sale_at")?,
        },
        revision_event_id: row.try_get("revision_event_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

impl Db {
    /// One page of CRM parties.
    #[datastore_span(name = "crm_list_parties", system = "postgresql")]
    pub async fn crm_list_parties(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        query: &PartyQuery,
    ) -> Result<PartyPage> {
        list_parties(&self.pool, community, space_channel_id, query).await
    }

    /// One CRM party.
    #[datastore_span(name = "crm_get_party", system = "postgresql")]
    pub async fn crm_get_party(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        party_id: Uuid,
    ) -> Result<Option<CrmPartyRecord>> {
        get_party(&self.pool, community, space_channel_id, party_id).await
    }
}
