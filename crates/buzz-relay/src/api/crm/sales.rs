//! `GET /crm/sales`, `GET /crm/sales/{id}`, `GET /crm/analytics` and
//! `GET /crm/payroll`.

use std::sync::Arc;

use axum::{
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use chrono::{Duration, NaiveDate};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use buzz_db::crm::{AnalyticsPeriod, SaleQuery, MAX_SALE_PAGE};

use super::authorize_space_member;
use crate::{
    api::{api_error, internal_error, not_found, parse_query_or_400},
    state::AppState,
};

/// Route path of the sale journal.
pub const SALES_PATH: &str = "/crm/sales";
/// Route path of one sale.
pub const SALE_PATH: &str = "/crm/sales/{sale_id}";
/// Route path of the cash-register shifts read.
pub const SHIFTS_PATH: &str = "/crm/shifts";
/// Route path of the analytics read.
pub const ANALYTICS_PATH: &str = "/crm/analytics";
/// Visit analytics: staff, services and clients.
pub const VISIT_ANALYTICS_PATH: &str = "/crm/analytics/services";
/// Staff payroll: accruals and payouts.
pub const PAYROLL_PATH: &str = "/crm/payroll";

const DEFAULT_SALE_LIMIT: i64 = 50;
/// Days in the default analytics period, ending today.
const DEFAULT_PERIOD_DAYS: i64 = 30;
/// Longest analytics period.
const MAX_PERIOD_DAYS: i64 = 366;

/// Query parameters of `GET /crm/sales`.
#[derive(Debug, Deserialize, Default)]
pub struct SalesParams {
    customer: Option<Uuid>,
    voided: Option<bool>,
    limit: Option<i64>,
    before: Option<i64>,
}

fn sale_query(params: SalesParams) -> Result<SaleQuery, &'static str> {
    let limit = params.limit.unwrap_or(DEFAULT_SALE_LIMIT);
    if !(1..=MAX_SALE_PAGE).contains(&limit) {
        return Err("limit must be between 1 and 200");
    }
    if params.before.is_some_and(|before| before < 1) {
        return Err("before must be a sale number");
    }
    Ok(SaleQuery {
        customer_id: params.customer,
        include_voided: params.voided.unwrap_or(true),
        before: params.before,
        limit,
        ..SaleQuery::default()
    })
}

/// `GET /crm/sales` — the sale journal, newest first.
///
/// `customer` narrows to one buyer, `voided=false` hides voided sales,
/// `before` is the `next` cursor (a sale number).
pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, SALES_PATH, raw_query.as_deref()).await {
            Ok(member) => member,
            Err(response) => return response,
        };
    let params: SalesParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    let query = match sale_query(params) {
        Ok(query) => query,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, message).into_response(),
    };
    match state
        .db
        .crm_list_sales(member.community(), member.space.channel_id, &query)
        .await
    {
        Ok(page) => Json(json!({ "sales": page.sales, "next": page.next })).into_response(),
        Err(e) => internal_error(&format!("CRM sales read: {e}")).into_response(),
    }
}

/// `GET /crm/sales/{id}` — one sale with its lines.
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(sale_id): Path<Uuid>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let path = format!("{SALES_PATH}/{sale_id}");
    let member = match authorize_space_member(&state, &headers, &path, raw_query.as_deref()).await {
        Ok(member) => member,
        Err(response) => return response,
    };
    match state
        .db
        .crm_get_sale(member.community(), member.space.channel_id, sale_id)
        .await
    {
        Ok(Some(sale)) => Json::<Value>(json!(sale)).into_response(),
        Ok(None) => not_found("sale not found").into_response(),
        Err(e) => internal_error(&format!("CRM sale read: {e}")).into_response(),
    }
}

/// `GET /crm/shifts` — the space's 20 most recent cash-register shifts with
/// their receipt totals, newest first.
pub async fn shifts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, SHIFTS_PATH, raw_query.as_deref()).await {
            Ok(member) => member,
            Err(response) => return response,
        };
    match state
        .db
        .crm_list_shifts(member.community(), member.space.channel_id, 20)
        .await
    {
        Ok(shifts) => Json(json!({ "shifts": shifts })).into_response(),
        Err(e) => internal_error(&format!("CRM shifts read: {e}")).into_response(),
    }
}

/// Query parameters of `GET /crm/analytics`.
#[derive(Debug, Deserialize, Default)]
pub struct AnalyticsParams {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
}

/// Resolve the period: missing ends default to the last
/// [`DEFAULT_PERIOD_DAYS`] days ending `today`.
fn period_bounds(
    params: &AnalyticsParams,
    today: NaiveDate,
) -> Result<(NaiveDate, NaiveDate), &'static str> {
    let to = params.to.unwrap_or(today);
    let from = params
        .from
        .unwrap_or(to - Duration::days(DEFAULT_PERIOD_DAYS - 1));
    if from > to {
        return Err("from must not be after to");
    }
    if (to - from).num_days() >= MAX_PERIOD_DAYS {
        return Err("the period must be at most 366 days");
    }
    Ok((from, to))
}

/// `GET /crm/analytics?from=YYYY-MM-DD&to=YYYY-MM-DD` — sales and customer
/// analytics for whole days in the space timezone (default: the last 30 days).
pub async fn analytics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member = match authorize_space_member(
        &state,
        &headers,
        ANALYTICS_PATH,
        raw_query.as_deref(),
    )
    .await
    {
        Ok(member) => member,
        Err(response) => return response,
    };
    let params: AnalyticsParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    let timezone = member.space.settings["timezone"]
        .as_str()
        .unwrap_or("UTC")
        .to_owned();
    let today = match state.db.crm_today_in(&timezone).await {
        Ok(today) => today,
        Err(e) => return internal_error(&format!("CRM analytics clock: {e}")).into_response(),
    };
    let (from, to) = match period_bounds(&params, today) {
        Ok(bounds) => bounds,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, message).into_response(),
    };
    let period = AnalyticsPeriod {
        from,
        to,
        timezone: timezone.clone(),
    };
    match state
        .db
        .crm_sales_analytics(member.community(), member.space.channel_id, &period)
        .await
    {
        Ok(analytics) => {
            let mut body = json!(analytics);
            body["from"] = json!(from);
            body["to"] = json!(to);
            body["timezone"] = json!(timezone);
            Json(body).into_response()
        }
        Err(e) => internal_error(&format!("CRM analytics read: {e}")).into_response(),
    }
}

/// `GET /crm/analytics/services?from&to` — booked visits per staff member,
/// the most booked services and client counts, for the same periods as
/// `GET /crm/analytics`.
pub async fn visit_analytics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, VISIT_ANALYTICS_PATH, raw_query.as_deref())
            .await
        {
            Ok(member) => member,
            Err(response) => return response,
        };
    let params: AnalyticsParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    let timezone = member.space.settings["timezone"]
        .as_str()
        .unwrap_or("UTC")
        .to_owned();
    let today = match state.db.crm_today_in(&timezone).await {
        Ok(today) => today,
        Err(e) => return internal_error(&format!("CRM analytics clock: {e}")).into_response(),
    };
    let (from, to) = match period_bounds(&params, today) {
        Ok(bounds) => bounds,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, message).into_response(),
    };
    let period = AnalyticsPeriod { from, to, timezone };
    match state
        .db
        .crm_visit_analytics(member.community(), member.space.channel_id, &period)
        .await
    {
        Ok(analytics) => {
            let mut body = json!(analytics);
            body["from"] = json!(from);
            body["to"] = json!(to);
            Json(body).into_response()
        }
        Err(e) => internal_error(&format!("CRM visit analytics read: {e}")).into_response(),
    }
}

/// `GET /crm/payroll?from&to` — what each staff member earned in the period
/// at their current rates and what they were paid. Owners and admins only:
/// pay is not for every member's eyes.
pub async fn payroll(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, PAYROLL_PATH, raw_query.as_deref()).await {
            Ok(member) => member,
            Err(response) => return response,
        };
    match super::space_role(&state, &member.reader, member.space.channel_id).await {
        Ok(Some(role)) if buzz_crm::decide(role, buzz_crm::CrmAction::ManageSpace) => {}
        Ok(_) => {
            return api_error(
                StatusCode::FORBIDDEN,
                "only space owners and admins see payroll",
            )
            .into_response()
        }
        Err(response) => return response,
    }
    let params: AnalyticsParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    let timezone = member.space.settings["timezone"]
        .as_str()
        .unwrap_or("UTC")
        .to_owned();
    let today = match state.db.crm_today_in(&timezone).await {
        Ok(today) => today,
        Err(e) => return internal_error(&format!("CRM payroll clock: {e}")).into_response(),
    };
    let (from, to) = match period_bounds(&params, today) {
        Ok(bounds) => bounds,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, message).into_response(),
    };
    match state
        .db
        .crm_payroll(member.community(), member.space.channel_id, from, to, today)
        .await
    {
        Ok(payroll) => Json(payroll_body(from, to, payroll)).into_response(),
        Err(e) => internal_error(&format!("CRM payroll read: {e}")).into_response(),
    }
}

fn payroll_body(
    from: NaiveDate,
    to: NaiveDate,
    payroll: buzz_db::crm::payroll::PayrollRecord,
) -> Value {
    let staff: Vec<Value> = payroll
        .staff
        .into_iter()
        .map(|row| {
            let rates = buzz_crm::PayRatesPayload {
                v: 1,
                staff_id: row.staff_id,
                percent_bp: u32::try_from(row.percent_bp).unwrap_or(0),
                per_visit_minor: row.per_visit_minor,
                shift_minor: row.shift_minor,
            };
            let accrued =
                buzz_crm::payroll::accrued_minor(&rates, row.revenue_minor, row.visits, row.shifts);
            let mut value = json!(row);
            value["accrued_minor"] = json!(accrued);
            value["balance_minor"] = json!(accrued.saturating_sub(row.paid_minor));
            value
        })
        .collect();
    json!({
        "from": from,
        "to": to,
        "staff": staff,
        "payouts": payroll.payouts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(raw: &str) -> NaiveDate {
        raw.parse().unwrap()
    }

    #[test]
    fn sale_query_defaults_and_bounds() {
        let query = sale_query(SalesParams::default()).unwrap();
        assert_eq!(query.limit, DEFAULT_SALE_LIMIT);
        assert!(query.include_voided);
        for bad in [0, MAX_SALE_PAGE + 1] {
            let params = SalesParams {
                limit: Some(bad),
                ..SalesParams::default()
            };
            assert!(sale_query(params).is_err());
        }
        let params = SalesParams {
            before: Some(0),
            ..SalesParams::default()
        };
        assert!(sale_query(params).is_err());
    }

    #[test]
    fn period_defaults_to_the_last_thirty_days() {
        let today = date("2026-10-25");
        assert_eq!(
            period_bounds(&AnalyticsParams::default(), today).unwrap(),
            (date("2026-09-26"), today)
        );
        let params = AnalyticsParams {
            from: Some(date("2026-10-01")),
            to: Some(date("2026-10-01")),
        };
        assert_eq!(
            period_bounds(&params, today).unwrap(),
            (date("2026-10-01"), date("2026-10-01"))
        );
    }

    #[test]
    fn period_must_be_ordered_and_bounded() {
        let today = date("2026-10-25");
        let reversed = AnalyticsParams {
            from: Some(date("2026-10-02")),
            to: Some(date("2026-10-01")),
        };
        assert!(period_bounds(&reversed, today).is_err());
        let too_long = AnalyticsParams {
            from: Some(date("2025-01-01")),
            to: Some(date("2026-01-02")),
        };
        assert!(period_bounds(&too_long, today).is_err());
        let full_year = AnalyticsParams {
            from: Some(date("2025-01-01")),
            to: Some(date("2026-01-01")),
        };
        assert!(period_bounds(&full_year, today).is_ok());
    }
}
