//! CRM sales and customer analytics over `crm_sales` (migration 0053).
//!
//! Every figure excludes voided sales. Day and hour boundaries are taken in
//! the space timezone, so "today" in a shop in Perm ends at Perm midnight.

use buzz_datastore_tracing::datastore_span;
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::{PgPool, Row as _};
use uuid::Uuid;

use crate::{error::Result, CommunityId, Db};

/// Rows in the top-products and top-customers lists.
pub const TOP_LIMIT: i64 = 10;

/// Analytics period: whole days `from..=to` in `timezone`.
#[derive(Debug, Clone)]
pub struct AnalyticsPeriod {
    /// First day.
    pub from: NaiveDate,
    /// Last day (inclusive).
    pub to: NaiveDate,
    /// IANA timezone of the space.
    pub timezone: String,
}

/// Totals for the period.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SalesTotals {
    /// Revenue after discounts, kopecks.
    pub revenue_minor: i64,
    /// Number of sales.
    pub sales_count: i64,
    /// Revenue / sales, rounded half-up to kopecks (0 without sales).
    pub avg_check_minor: i64,
    /// Discounts given, kopecks.
    pub discount_minor: i64,
}

/// Revenue in one bucket (day, hour or payment method).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RevenueBucket<K> {
    /// Bucket key.
    pub key: K,
    /// Revenue, kopecks.
    pub revenue_minor: i64,
    /// Number of sales.
    pub sales_count: i64,
}

/// One row of the top-products list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopProduct {
    /// Product id.
    pub product_id: Uuid,
    /// Current catalog name.
    pub name: String,
    /// Unit wire name.
    pub unit: String,
    /// Quantity sold, thousandths.
    pub qty_milli: i64,
    /// Revenue, kopecks.
    pub revenue_minor: i64,
}

/// One row of the top-customers list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopCustomer {
    /// Party id.
    pub customer_id: Uuid,
    /// Current name.
    pub name: String,
    /// Revenue, kopecks.
    pub revenue_minor: i64,
    /// Number of sales.
    pub sales_count: i64,
}

/// Customer figures for the period.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CustomerFigures {
    /// Distinct identified buyers.
    pub buyers_count: i64,
    /// Identified buyers with two or more sales in the period.
    pub repeat_buyers_count: i64,
    /// Identified buyers whose first ever sale falls in the period.
    pub new_buyers_count: i64,
    /// Revenue from sales with a customer, kopecks.
    pub identified_revenue_minor: i64,
    /// Revenue from anonymous sales, kopecks.
    pub anonymous_revenue_minor: i64,
}

/// `GET /crm/analytics` read model.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SalesAnalytics {
    /// Period totals.
    pub totals: SalesTotals,
    /// Days with sales, ascending.
    pub by_day: Vec<RevenueBucket<NaiveDate>>,
    /// Hours of day (0–23) with sales, ascending.
    pub by_hour: Vec<RevenueBucket<i32>>,
    /// Payment methods with sales, by revenue.
    pub by_payment: Vec<RevenueBucket<String>>,
    /// Best-selling products by revenue.
    pub top_products: Vec<TopProduct>,
    /// Best customers by revenue.
    pub top_customers: Vec<TopCustomer>,
    /// Customer figures.
    pub customers: CustomerFigures,
}

/// Restricts `crm_sales s` to the period. Binds `$1` community, `$2` space,
/// `$3` first day, `$4` last day, `$5` timezone.
macro_rules! in_period {
    () => {
        " s.community_id = $1 AND s.space_channel_id = $2 AND s.voided_at IS NULL \
          AND s.sold_at >= ($3::date::timestamp AT TIME ZONE $5) \
          AND s.sold_at < (($4::date + 1)::timestamp AT TIME ZONE $5) "
    };
}

/// Revenue / count, rounded half-up.
fn average(revenue_minor: i64, count: i64) -> i64 {
    if count <= 0 {
        return 0;
    }
    let (revenue, count) = (i128::from(revenue_minor), i128::from(count));
    i64::try_from((2 * revenue + count) / (2 * count)).unwrap_or(i64::MAX)
}

/// Compute the analytics for one period.
pub async fn sales_analytics(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    period: &AnalyticsPeriod,
) -> Result<SalesAnalytics> {
    macro_rules! query {
        ($sql:expr) => {
            sqlx::query($sql)
                .bind(community.as_uuid())
                .bind(space_channel_id)
                .bind(period.from)
                .bind(period.to)
                .bind(&period.timezone)
        };
    }

    let totals = query!(concat!(
        "SELECT COALESCE(sum(total_minor), 0)::bigint AS revenue, count(*)::bigint AS sales, \
                COALESCE(sum(discount_minor), 0)::bigint AS discount, \
                COALESCE(sum(total_minor) FILTER (WHERE customer_id IS NOT NULL), 0)::bigint \
                    AS identified, \
                count(DISTINCT customer_id)::bigint AS buyers \
         FROM crm_sales s WHERE",
        in_period!()
    ))
    .fetch_one(pool)
    .await?;
    let revenue: i64 = totals.try_get("revenue")?;
    let sales: i64 = totals.try_get("sales")?;
    let identified: i64 = totals.try_get("identified")?;

    let bucket_rows =
        |rows: Vec<sqlx::postgres::PgRow>| -> Result<Vec<(sqlx::postgres::PgRow, i64, i64)>> {
            rows.into_iter()
                .map(|row| {
                    let revenue: i64 = row.try_get("revenue")?;
                    let sales: i64 = row.try_get("sales")?;
                    Ok((row, revenue, sales))
                })
                .collect()
        };

    let by_day = bucket_rows(
        query!(concat!(
            "SELECT (s.sold_at AT TIME ZONE $5)::date AS key, \
                    sum(total_minor)::bigint AS revenue, count(*)::bigint AS sales \
             FROM crm_sales s WHERE",
            in_period!(),
            "GROUP BY 1 ORDER BY 1"
        ))
        .fetch_all(pool)
        .await?,
    )?
    .into_iter()
    .map(|(row, revenue_minor, sales_count)| {
        Ok(RevenueBucket {
            key: row.try_get::<NaiveDate, _>("key")?,
            revenue_minor,
            sales_count,
        })
    })
    .collect::<Result<Vec<_>>>()?;

    let by_hour = bucket_rows(
        query!(concat!(
            "SELECT extract(hour FROM s.sold_at AT TIME ZONE $5)::int AS key, \
                    sum(total_minor)::bigint AS revenue, count(*)::bigint AS sales \
             FROM crm_sales s WHERE",
            in_period!(),
            "GROUP BY 1 ORDER BY 1"
        ))
        .fetch_all(pool)
        .await?,
    )?
    .into_iter()
    .map(|(row, revenue_minor, sales_count)| {
        Ok(RevenueBucket {
            key: row.try_get::<i32, _>("key")?,
            revenue_minor,
            sales_count,
        })
    })
    .collect::<Result<Vec<_>>>()?;

    let by_payment = bucket_rows(
        query!(concat!(
            "SELECT s.payment AS key, sum(total_minor)::bigint AS revenue, \
                    count(*)::bigint AS sales \
             FROM crm_sales s WHERE",
            in_period!(),
            "GROUP BY 1 ORDER BY 2 DESC, 1"
        ))
        .fetch_all(pool)
        .await?,
    )?
    .into_iter()
    .map(|(row, revenue_minor, sales_count)| {
        Ok(RevenueBucket {
            key: row.try_get::<String, _>("key")?,
            revenue_minor,
            sales_count,
        })
    })
    .collect::<Result<Vec<_>>>()?;

    let top_products = query!(concat!(
        "SELECT l.product_id, p.name, p.unit, sum(l.qty_milli)::bigint AS qty, \
                sum(l.amount_minor)::bigint AS revenue \
         FROM crm_sales s \
         JOIN crm_sale_lines l ON l.community_id = s.community_id AND l.sale_id = s.sale_id \
         JOIN crm_products p ON p.community_id = l.community_id AND p.product_id = l.product_id \
         WHERE",
        in_period!(),
        "GROUP BY l.product_id, p.name, p.unit \
         ORDER BY revenue DESC, l.product_id LIMIT $6"
    ))
    .bind(TOP_LIMIT)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(TopProduct {
            product_id: row.try_get("product_id")?,
            name: row.try_get("name")?,
            unit: row.try_get("unit")?,
            qty_milli: row.try_get("qty")?,
            revenue_minor: row.try_get("revenue")?,
        })
    })
    .collect::<Result<Vec<_>>>()?;

    let top_customers = query!(concat!(
        "SELECT s.customer_id, p.name, sum(s.total_minor)::bigint AS revenue, \
                count(*)::bigint AS sales \
         FROM crm_sales s \
         JOIN crm_parties p ON p.community_id = s.community_id AND p.party_id = s.customer_id \
         WHERE",
        in_period!(),
        "GROUP BY s.customer_id, p.name \
         ORDER BY revenue DESC, s.customer_id LIMIT $6"
    ))
    .bind(TOP_LIMIT)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(TopCustomer {
            customer_id: row.try_get("customer_id")?,
            name: row.try_get("name")?,
            revenue_minor: row.try_get("revenue")?,
            sales_count: row.try_get("sales")?,
        })
    })
    .collect::<Result<Vec<_>>>()?;

    let buyers = query!(concat!(
        "WITH period AS ( \
             SELECT s.customer_id, count(*) AS sales FROM crm_sales s WHERE",
        in_period!(),
        "AND s.customer_id IS NOT NULL GROUP BY s.customer_id) \
         SELECT count(*) FILTER (WHERE sales >= 2)::bigint AS repeat, \
                count(*) FILTER (WHERE NOT EXISTS ( \
                    SELECT 1 FROM crm_sales e \
                    WHERE e.community_id = $1 AND e.customer_id = period.customer_id \
                      AND e.voided_at IS NULL \
                      AND e.sold_at < ($3::date::timestamp AT TIME ZONE $5)))::bigint AS new \
         FROM period"
    ))
    .fetch_one(pool)
    .await?;

    Ok(SalesAnalytics {
        totals: SalesTotals {
            revenue_minor: revenue,
            sales_count: sales,
            avg_check_minor: average(revenue, sales),
            discount_minor: totals.try_get("discount")?,
        },
        by_day,
        by_hour,
        by_payment,
        top_products,
        top_customers,
        customers: CustomerFigures {
            buyers_count: totals.try_get("buyers")?,
            repeat_buyers_count: buyers.try_get("repeat")?,
            new_buyers_count: buyers.try_get("new")?,
            identified_revenue_minor: identified,
            anonymous_revenue_minor: revenue - identified,
        },
    })
}

/// Today's date in `timezone`.
pub async fn today_in(pool: &PgPool, timezone: &str) -> Result<NaiveDate> {
    Ok(sqlx::query_scalar("SELECT (now() AT TIME ZONE $1)::date")
        .bind(timezone)
        .fetch_one(pool)
        .await?)
}

impl Db {
    /// Today's date in the given IANA timezone.
    #[datastore_span(name = "crm_today_in", system = "postgresql")]
    pub async fn crm_today_in(&self, timezone: &str) -> Result<NaiveDate> {
        today_in(&self.pool, timezone).await
    }

    /// Sales and customer analytics of a CRM space for one period.
    #[datastore_span(name = "crm_sales_analytics", system = "postgresql")]
    pub async fn crm_sales_analytics(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        period: &AnalyticsPeriod,
    ) -> Result<SalesAnalytics> {
        sales_analytics(&self.pool, community, space_channel_id, period).await
    }
}

#[cfg(test)]
mod tests {
    use super::average;

    #[test]
    fn average_rounds_half_up() {
        assert_eq!(average(0, 0), 0);
        assert_eq!(average(1_000, 3), 333);
        assert_eq!(average(1_001, 2), 501);
        assert_eq!(average(999, 2), 500);
    }
}
