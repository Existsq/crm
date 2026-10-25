//! Salon analytics over booked visits (`crm_appointments`, migration 0056)
//! and working days (`crm_staff_days`, 0055): per staff member, per service
//! and for clients.
//!
//! A visit counts when it holds its time (not cancelled, not missed); its
//! revenue counts once it is done, at the prices agreed for the visit.

use buzz_datastore_tracing::datastore_span;
use serde::Serialize;
use sqlx::{PgPool, Row as _};
use uuid::Uuid;

use super::analytics::AnalyticsPeriod;
use crate::{error::Result, CommunityId, Db};

/// Rows in the top-services list.
pub const TOP_SERVICES: i64 = 10;

/// One staff member's figures for the period.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StaffVisitStats {
    /// Staff id.
    pub staff_id: Uuid,
    /// Current name.
    pub name: String,
    /// Visits holding time.
    pub visits: i64,
    /// Visits done.
    pub done: i64,
    /// Missed visits.
    pub no_shows: i64,
    /// Cancelled visits.
    pub cancelled: i64,
    /// Revenue of done visits, kopecks.
    pub revenue_minor: i64,
    /// Booked minutes of visits holding time.
    pub booked_min: i64,
    /// Scheduled working minutes.
    pub worked_min: i64,
}

/// One service's figures for the period.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ServiceVisitStats {
    /// Product id.
    pub product_id: Uuid,
    /// Name as booked.
    pub name: String,
    /// Times booked in visits holding time.
    pub visits: i64,
    /// Booked prices, kopecks.
    pub revenue_minor: i64,
    /// Booked minutes.
    pub minutes: i64,
}

/// Client figures for the period.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct VisitClients {
    /// Distinct client cards with a visit.
    pub clients: i64,
    /// Of them, clients with no earlier visit.
    pub new_clients: i64,
    /// Visits without a client card.
    pub walk_ins: i64,
}

/// `GET /crm/analytics/services` body without the period echo.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VisitAnalytics {
    /// Active staff, and archived staff with visits in the period.
    pub staff: Vec<StaffVisitStats>,
    /// Most booked services.
    pub services: Vec<ServiceVisitStats>,
    /// Client figures.
    pub clients: VisitClients,
}

macro_rules! holds_time {
    ($alias:literal) => {
        concat!($alias, ".status NOT IN ('no_show', 'cancelled')")
    };
}

/// Visit analytics of a space for `period`.
pub async fn visit_analytics(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    period: &AnalyticsPeriod,
) -> Result<VisitAnalytics> {
    let staff_rows = sqlx::query(concat!(
        "WITH worked AS ( \
             SELECT d.staff_id, \
                    COALESCE(sum((i->>'end')::int - (i->>'start')::int), 0)::BIGINT AS minutes \
             FROM crm_staff_days d CROSS JOIN LATERAL jsonb_array_elements(d.intervals) i \
             WHERE d.community_id = $1 AND d.day BETWEEN $3 AND $4 \
             GROUP BY d.staff_id \
         ), visits AS ( \
             SELECT a.staff_id, \
                    count(*) FILTER (WHERE ",
        holds_time!("a"),
        ") AS visits, \
                    count(*) FILTER (WHERE a.status = 'done') AS done, \
                    count(*) FILTER (WHERE a.status = 'no_show') AS no_shows, \
                    count(*) FILTER (WHERE a.status = 'cancelled') AS cancelled, \
                    COALESCE(sum(a.total_minor) FILTER (WHERE a.status = 'done'), 0)::BIGINT \
                        AS revenue_minor, \
                    COALESCE(sum(a.end_min - a.start_min) FILTER (WHERE ",
        holds_time!("a"),
        "), 0)::BIGINT AS booked_min \
             FROM crm_appointments a \
             WHERE a.community_id = $1 AND a.space_channel_id = $2 \
               AND a.day BETWEEN $3 AND $4 \
             GROUP BY a.staff_id \
         ) \
         SELECT s.staff_id, s.name, \
                COALESCE(v.visits, 0) AS visits, COALESCE(v.done, 0) AS done, \
                COALESCE(v.no_shows, 0) AS no_shows, COALESCE(v.cancelled, 0) AS cancelled, \
                COALESCE(v.revenue_minor, 0) AS revenue_minor, \
                COALESCE(v.booked_min, 0) AS booked_min, \
                COALESCE(w.minutes, 0) AS worked_min \
         FROM crm_staff s \
         LEFT JOIN visits v ON v.staff_id = s.staff_id \
         LEFT JOIN worked w ON w.staff_id = s.staff_id \
         WHERE s.community_id = $1 AND s.space_channel_id = $2 \
           AND (s.archived_at IS NULL OR v.staff_id IS NOT NULL) \
         ORDER BY revenue_minor DESC, visits DESC, s.name, s.staff_id"
    ))
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(period.from)
    .bind(period.to)
    .fetch_all(pool)
    .await?;
    let staff = staff_rows
        .iter()
        .map(|row| {
            Ok(StaffVisitStats {
                staff_id: row.try_get("staff_id")?,
                name: row.try_get("name")?,
                visits: row.try_get("visits")?,
                done: row.try_get("done")?,
                no_shows: row.try_get("no_shows")?,
                cancelled: row.try_get("cancelled")?,
                revenue_minor: row.try_get("revenue_minor")?,
                booked_min: row.try_get("booked_min")?,
                worked_min: row.try_get("worked_min")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let service_rows = sqlx::query(concat!(
        "SELECT (e->>'product_id')::uuid AS product_id, max(e->>'name') AS name, \
                count(*) AS visits, \
                COALESCE(sum((e->>'price_minor')::bigint), 0)::BIGINT AS revenue_minor, \
                COALESCE(sum((e->>'duration_min')::bigint), 0)::BIGINT AS minutes \
         FROM crm_appointments a CROSS JOIN LATERAL jsonb_array_elements(a.services) e \
         WHERE a.community_id = $1 AND a.space_channel_id = $2 \
           AND a.day BETWEEN $3 AND $4 AND ",
        holds_time!("a"),
        " GROUP BY 1 ORDER BY visits DESC, revenue_minor DESC, name LIMIT $5"
    ))
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(period.from)
    .bind(period.to)
    .bind(TOP_SERVICES)
    .fetch_all(pool)
    .await?;
    let services = service_rows
        .iter()
        .map(|row| {
            Ok(ServiceVisitStats {
                product_id: row.try_get("product_id")?,
                name: row.try_get("name")?,
                visits: row.try_get("visits")?,
                revenue_minor: row.try_get("revenue_minor")?,
                minutes: row.try_get("minutes")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let clients = sqlx::query(concat!(
        "WITH period AS ( \
             SELECT a.customer_id FROM crm_appointments a \
             WHERE a.community_id = $1 AND a.space_channel_id = $2 \
               AND a.day BETWEEN $3 AND $4 AND ",
        holds_time!("a"),
        " ) \
         SELECT count(DISTINCT p.customer_id) AS clients, \
                count(DISTINCT p.customer_id) FILTER (WHERE NOT EXISTS ( \
                    SELECT 1 FROM crm_appointments b \
                    WHERE b.community_id = $1 AND b.space_channel_id = $2 \
                      AND b.customer_id = p.customer_id AND b.day < $3 AND ",
        holds_time!("b"),
        " )) AS new_clients, \
                count(*) FILTER (WHERE p.customer_id IS NULL) AS walk_ins \
         FROM period p"
    ))
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(period.from)
    .bind(period.to)
    .fetch_one(pool)
    .await?;
    Ok(VisitAnalytics {
        staff,
        services,
        clients: VisitClients {
            clients: clients.try_get("clients")?,
            new_clients: clients.try_get("new_clients")?,
            walk_ins: clients.try_get("walk_ins")?,
        },
    })
}

impl Db {
    /// Visit analytics of a CRM space for one period.
    #[datastore_span(name = "crm_visit_analytics", system = "postgresql")]
    pub async fn crm_visit_analytics(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        period: &AnalyticsPeriod,
    ) -> Result<VisitAnalytics> {
        visit_analytics(&self.pool, community, space_channel_id, period).await
    }
}
