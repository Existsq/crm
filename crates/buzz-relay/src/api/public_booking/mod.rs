//! Public online booking: the page clients book on, with no account.
//!
//! - `GET /book` — the booking page (static HTML, same origin as the API).
//! - `GET /public/booking` — salon, services and staff the page offers.
//! - `GET /public/booking/slots?date&service_ids[&staff_id]` — free starts.
//! - `POST /public/booking/code` — text a six-digit code to the client.
//! - `POST /public/booking/appointments` — book with that code.
//!
//! The community is bound from the host like every other route. Everything
//! answers the same 404 unless the community's CRM space has enabled online
//! booking (kind 42135). Bookings are relay-signed kind 42134 events and go
//! through the appointment checks of the journal (hours, overlap, services),
//! priced from the catalog. Every route is rate-limited per client address;
//! codes are also limited per phone and per community in the database.

// Response is the natural error type for axum handlers and their helpers.
#![allow(clippy::result_large_err)]

pub mod sms;

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{ConnectInfo, RawQuery, State},
    http::{header, Extensions, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Json, Response},
};
use chrono::{Duration as ChronoDuration, NaiveDate, NaiveDateTime};
use hmac::{Hmac, KeyInit, Mac};
use nostr::{EventBuilder, Kind, Tag};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Sha256;
use uuid::Uuid;

use buzz_core::kind::KIND_CRM_ONLINE_BOOKING;
use buzz_core::{CommunityId, TenantContext};
use buzz_crm::booking::{client_phone, CODE_DIGITS};
use buzz_crm::{
    format_clock, parse_clock, tags, validate, AppointmentPayload, AppointmentService,
    AppointmentSource, AppointmentStatus, Money, OnlineBookingRequest, OnlineBookingSettings,
};
use buzz_db::crm::{CodeCheck, CodeIssue, CodeIssueOutcome, CodeLimits, CrmSpaceRecord};

use crate::api::crm::appointments::{slot_request, slots_json, SlotParams};
use crate::api::{internal_error, parse_query_or_400};
use crate::handlers::crm::booking::{ingest_online_booking, OnlineBookingError};
use crate::handlers::ingest::IngestError;
use crate::state::AppState;

pub use sms::BookingConfig;

/// Route of the booking page.
pub const PAGE_PATH: &str = "/book";
/// Route of the page data.
pub const INFO_PATH: &str = "/public/booking";
/// Route of the free starts.
pub const SLOTS_PATH: &str = "/public/booking/slots";
/// Route that texts a code.
pub const CODE_PATH: &str = "/public/booking/code";
/// Route that books.
pub const BOOK_PATH: &str = "/public/booking/appointments";

/// The booking page, served as is.
const PAGE_HTML: &str = include_str!("page.html");
/// The page loads nothing from elsewhere.
const PAGE_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; \
    style-src 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; \
    base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// Lifetime of a code.
const CODE_TTL_SECS: i64 = 10 * 60;
/// Wrong attempts before a code locks.
const CODE_MAX_ATTEMPTS: i32 = 5;
/// Database limits on sending codes.
const CODE_LIMITS: CodeLimits = CodeLimits {
    resend_after_secs: 60,
    per_phone_per_day: 5,
    per_community_per_hour: 100,
};

/// Window of the per-address limits.
pub const RATE_WINDOW: Duration = Duration::from_secs(60 * 60);
/// Addresses tracked at once; the oldest are evicted first.
pub const RATE_CACHE_CAPACITY: u64 = 100_000;

/// What a client address is doing, each with its own hourly allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateBucket {
    /// Page data and free starts.
    Read,
    /// Texting codes.
    Code,
    /// Booking attempts.
    Book,
}

impl RateBucket {
    const fn per_hour(self) -> u32 {
        match self {
            RateBucket::Read => 1200,
            RateBucket::Code => 10,
            RateBucket::Book => 30,
        }
    }
}

/// Key of the per-address limiter.
pub type RateKey = (CommunityId, IpAddr, RateBucket);
/// Fixed-window counters per (community, address, bucket).
pub type RateLimiter = moka::sync::Cache<RateKey, Arc<AtomicU32>>;

/// A new per-address limiter.
pub fn rate_limiter() -> RateLimiter {
    moka::sync::Cache::builder()
        .max_capacity(RATE_CACHE_CAPACITY)
        .time_to_live(RATE_WINDOW)
        .build()
}

fn rate_limited(limiter: &RateLimiter, key: RateKey) -> bool {
    let counter = limiter.get_with(key, || Arc::new(AtomicU32::new(0)));
    counter.fetch_add(1, Ordering::Relaxed) >= key.2.per_hour()
}

/// The client address: the socket peer, or the last `X-Forwarded-For` hop
/// when the relay is configured to sit behind one trusted proxy.
fn client_ip(trust_forwarded_for: bool, headers: &HeaderMap, peer: Option<SocketAddr>) -> IpAddr {
    let forwarded = trust_forwarded_for
        .then(|| {
            headers
                .get_all("x-forwarded-for")
                .iter()
                .filter_map(|value| value.to_str().ok())
                .flat_map(|value| value.split(','))
                .filter_map(|hop| hop.trim().parse::<IpAddr>().ok())
                .next_back()
        })
        .flatten();
    forwarded
        .or_else(|| peer.map(|addr| addr.ip()))
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

/// A public error: a stable `error` code for the page plus a message.
fn public_error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "error": code, "message": message }))).into_response()
}

fn unavailable() -> Response {
    public_error(
        StatusCode::NOT_FOUND,
        "unavailable",
        "online booking is not available here",
    )
}

/// The space and its enabled booking settings, resolved for one request.
struct Page {
    tenant: TenantContext,
    space: CrmSpaceRecord,
    settings: OnlineBookingSettings,
}

impl Page {
    fn community(&self) -> CommunityId {
        self.tenant.community()
    }

    fn timezone(&self) -> &str {
        self.space.settings["timezone"].as_str().unwrap_or("UTC")
    }
}

/// Bind the community from the host, charge the address `bucket`, and load
/// the enabled booking page. Every "no page here" answer is the same 404.
async fn open_page(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    bucket: RateBucket,
) -> Result<Page, Response> {
    let raw_host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let tenant = crate::tenant::bind_community(&state.db, raw_host)
        .await
        .map_err(|_| unavailable())?;
    let ip = client_ip(state.config.booking.trust_forwarded_for, headers, peer);
    if rate_limited(
        &state.public_booking_limiter,
        (tenant.community(), ip, bucket),
    ) {
        return Err(public_error(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many requests; try again later",
        ));
    }
    let space = state
        .db
        .crm_get_space(tenant.community())
        .await
        .map_err(|e| internal_error(&format!("booking space read: {e}")).into_response())?
        .ok_or_else(unavailable)?;
    let record = state
        .db
        .crm_get_online_booking(tenant.community(), space.channel_id)
        .await
        .map_err(|e| internal_error(&format!("booking settings read: {e}")).into_response())?
        .filter(|record| record.enabled)
        .ok_or_else(unavailable)?;
    let settings = OnlineBookingSettings::parse(&record.settings.to_string())
        .map_err(|e| internal_error(&format!("booking settings: {e}")).into_response())?;
    Ok(Page {
        tenant,
        space,
        settings,
    })
}

fn peer(extensions: &Extensions) -> Option<SocketAddr> {
    extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| *addr)
}

/// `GET /book` — the booking page. The page itself is static; it asks
/// `/public/booking` whether booking is open.
pub async fn page() -> Response {
    let mut response = Html(PAGE_HTML).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(PAGE_CSP),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// Services and staff the page offers: listed (or all), live, bookable, and
/// paired — a service nobody offered performs is hidden, and so is a staff
/// member with no offered service.
fn offer(
    page: &Page,
    services: Vec<buzz_db::crm::PublicService>,
    staff: Vec<buzz_db::crm::PublicStaff>,
) -> Value {
    let settings = &page.settings;
    let staff: Vec<_> = staff
        .into_iter()
        .filter(|member| settings.offers_staff(member.id))
        .map(|mut member| {
            member
                .service_ids
                .retain(|id| settings.offers_service(*id) && services.iter().any(|s| s.id == *id));
            member
        })
        .filter(|member| !member.service_ids.is_empty())
        .collect();
    let services: Vec<_> = services
        .into_iter()
        .filter(|service| {
            settings.offers_service(service.id)
                && staff
                    .iter()
                    .any(|member| member.service_ids.contains(&service.id))
        })
        .collect();
    json!({ "services": services, "staff": staff })
}

/// `GET /public/booking`.
pub async fn info(
    State(state): State<Arc<AppState>>,
    extensions: Extensions,
    headers: HeaderMap,
) -> Response {
    let page = match open_page(&state, &headers, peer(&extensions), RateBucket::Read).await {
        Ok(page) => page,
        Err(response) => return response,
    };
    let (services, staff) = match state
        .db
        .crm_public_catalog(page.community(), page.space.channel_id)
        .await
    {
        Ok(catalog) => catalog,
        Err(e) => return internal_error(&format!("booking catalog read: {e}")).into_response(),
    };
    let today = match local_now(&state, &page).await {
        Ok(now) => now.date(),
        Err(response) => return response,
    };
    let mut body = offer(&page, services, staff);
    let settings = &page.settings;
    body["title"] = json!(settings.title);
    body["about"] = json!(settings.about);
    body["address"] = json!(settings.address);
    body["phone"] = json!(settings.phone);
    body["currency"] = page.space.settings["currency"].clone();
    body["timezone"] = json!(page.timezone());
    body["today"] = json!(today);
    body["last_day"] = json!(last_day(today, settings));
    body["step_min"] = json!(settings.step_min);
    body["code_digits"] = json!(CODE_DIGITS);
    Json(body).into_response()
}

async fn local_now(state: &Arc<AppState>, page: &Page) -> Result<NaiveDateTime, Response> {
    state
        .db
        .crm_space_now_local(page.community(), page.space.channel_id)
        .await
        .map_err(|e| internal_error(&format!("booking clock read: {e}")).into_response())?
        .ok_or_else(unavailable)
}

fn last_day(today: NaiveDate, settings: &OnlineBookingSettings) -> NaiveDate {
    today + ChronoDuration::days(i64::from(settings.horizon_days))
}

/// The earliest bookable moment: now plus the minimum notice.
fn earliest(now_local: NaiveDateTime, settings: &OnlineBookingSettings) -> NaiveDateTime {
    now_local + ChronoDuration::minutes(i64::from(settings.min_notice_min))
}

/// `GET /public/booking/slots?date&service_ids[&staff_id]`.
pub async fn slots(
    State(state): State<Arc<AppState>>,
    extensions: Extensions,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let page = match open_page(&state, &headers, peer(&extensions), RateBucket::Read).await {
        Ok(page) => page,
        Err(response) => return response,
    };
    let mut params: SlotParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    // The salon sets the grid.
    params.step = u16::try_from(page.settings.step_min).ok();
    let request = match slot_request(&params) {
        Ok(request) => request,
        Err(message) => return public_error(StatusCode::BAD_REQUEST, "invalid", &message),
    };
    if let Some(problem) = offer_problem(&page.settings, request.staff_id, &request.service_ids) {
        return public_error(StatusCode::BAD_REQUEST, "invalid", problem);
    }
    let inputs = match state
        .db
        .crm_slot_inputs(
            page.community(),
            page.space.channel_id,
            request.date,
            &request.service_ids,
            request.staff_id,
        )
        .await
    {
        Ok(Some(inputs)) => inputs,
        Ok(None) => return unavailable(),
        Err(e) => return internal_error(&format!("booking slots read: {e}")).into_response(),
    };
    // Past the horizon nothing is free: every start is before the cutoff.
    let cutoff = if request.date > last_day(inputs.now_local.date(), &page.settings) {
        NaiveDateTime::MAX
    } else {
        earliest(inputs.now_local, &page.settings)
    };
    let mut body = match slots_json(&request, &inputs, cutoff) {
        Ok(body) => body,
        Err(message) => return public_error(StatusCode::BAD_REQUEST, "invalid", &message),
    };
    if let Some(staff) = body["staff"].as_array_mut() {
        staff.retain(|member| {
            member["staff_id"]
                .as_str()
                .and_then(|id| id.parse().ok())
                .is_some_and(|id| page.settings.offers_staff(id))
        });
    }
    Json(body).into_response()
}

/// Why the page does not offer this staff member or these services.
fn offer_problem(
    settings: &OnlineBookingSettings,
    staff_id: Option<Uuid>,
    service_ids: &[Uuid],
) -> Option<&'static str> {
    if staff_id.is_some_and(|id| !settings.offers_staff(id)) {
        return Some("staff_id is not offered online");
    }
    if service_ids.iter().any(|id| !settings.offers_service(*id)) {
        return Some("service_ids: a service is not offered online");
    }
    None
}

/// Body of `POST /public/booking/code`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeBody {
    phone: String,
}

fn code_hash(
    state: &AppState,
    community: CommunityId,
    verification_id: Uuid,
    code: &str,
) -> Result<[u8; 32], Response> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(state.relay_keypair.secret_key().as_secret_bytes())
            .map_err(|e| internal_error(&format!("booking code key: {e}")).into_response())?;
    mac.update(b"buzz-crm-booking-code:");
    mac.update(community.as_uuid().as_bytes());
    mac.update(verification_id.as_bytes());
    mac.update(code.as_bytes());
    Ok(mac.finalize().into_bytes().into())
}

fn sms_text(title: &str, code: &str) -> String {
    let salon: String = title.chars().take(40).collect();
    format!("{code} — код для онлайн-записи в «{salon}». Никому его не сообщайте.")
}

/// `POST /public/booking/code` — `{"phone"}` → `{"verification_id",
/// "expires_in", "resend_after"}`.
pub async fn send_code(
    State(state): State<Arc<AppState>>,
    extensions: Extensions,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let page = match open_page(&state, &headers, peer(&extensions), RateBucket::Code).await {
        Ok(page) => page,
        Err(response) => return response,
    };
    let body: CodeBody = match serde_json::from_slice(&body) {
        Ok(body) => body,
        Err(_) => return public_error(StatusCode::BAD_REQUEST, "invalid", "send {\"phone\"}"),
    };
    let (phone, phone_key) = match client_phone(&body.phone) {
        Ok(phone) => phone,
        Err(_) => {
            return public_error(
                StatusCode::BAD_REQUEST,
                "invalid_phone",
                "check the phone number",
            )
        }
    };
    let verification_id = Uuid::new_v4();
    let code = format!(
        "{:0width$}",
        rand::random_range(0..1_000_000u32),
        width = CODE_DIGITS
    );
    let hash = match code_hash(&state, page.community(), verification_id, &code) {
        Ok(hash) => hash,
        Err(response) => return response,
    };
    let issued = state
        .db
        .crm_issue_booking_code(
            page.community(),
            CodeIssue {
                verification_id,
                phone: &phone,
                phone_key: &phone_key,
                code_hash: &hash,
                ttl_secs: CODE_TTL_SECS,
            },
            CODE_LIMITS,
        )
        .await;
    match issued {
        Ok(CodeIssueOutcome::Issued) => {}
        Ok(CodeIssueOutcome::RetryAfter(secs)) => {
            let mut response = public_error(
                StatusCode::TOO_MANY_REQUESTS,
                "retry_later",
                "a code was just sent to this phone",
            );
            if let Ok(value) = HeaderValue::from_str(&secs.max(1).to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
            return response;
        }
        Ok(CodeIssueOutcome::PhoneLimit) => {
            return public_error(
                StatusCode::TOO_MANY_REQUESTS,
                "phone_limit",
                "too many codes for this phone today",
            )
        }
        Ok(CodeIssueOutcome::CommunityLimit) => {
            return public_error(
                StatusCode::TOO_MANY_REQUESTS,
                "busy",
                "too many codes right now; try again later",
            )
        }
        Err(e) => return internal_error(&format!("booking code write: {e}")).into_response(),
    }
    if let Err(error) = sms::send(
        &state.config.booking.sms,
        &phone_key,
        &sms_text(&page.settings.title, &code),
    )
    .await
    {
        tracing::warn!(%error, "booking SMS failed");
        if let Err(e) = state
            .db
            .crm_discard_booking_code(page.community(), verification_id)
            .await
        {
            tracing::warn!(error = %e, "booking code discard failed");
        }
        return public_error(
            StatusCode::BAD_GATEWAY,
            "sms_failed",
            "the SMS could not be sent; try again later",
        );
    }
    let mut response = json!({
        "verification_id": verification_id,
        "expires_in": CODE_TTL_SECS,
        "resend_after": CODE_LIMITS.resend_after_secs,
    });
    if state.config.booking.expose_codes {
        response["dev_code"] = json!(code);
    }
    Json(response).into_response()
}

/// A booking that passed every check made before signing.
struct Booking {
    payload: AppointmentPayload,
    staff_name: String,
    service_names: Vec<String>,
}

/// Check the request against the page and the catalog and price it.
fn plan_booking(
    page: &Page,
    request: &OnlineBookingRequest,
    inputs: &buzz_db::crm::SlotInputs,
    day: NaiveDate,
    start: u16,
    phone: String,
    customer_id: Option<Uuid>,
) -> Result<Booking, Response> {
    let invalid = |code: &str, message: &str| public_error(StatusCode::BAD_REQUEST, code, message);
    let settings = &page.settings;
    if let Some(problem) = offer_problem(settings, Some(request.staff_id), &request.service_ids) {
        return Err(invalid("invalid", problem));
    }
    let today = inputs.now_local.date();
    let at = day.and_time(chrono::NaiveTime::MIN) + ChronoDuration::minutes(i64::from(start));
    if day > last_day(today, settings) || at < earliest(inputs.now_local, settings) {
        return Err(invalid(
            "too_late",
            "this time can no longer be booked online",
        ));
    }
    if u32::from(start) % settings.step_min != 0 {
        return Err(invalid("invalid", "start is not on the booking grid"));
    }
    let Some((_, staff_name, _)) = inputs
        .staff
        .iter()
        .find(|(id, _, _)| *id == request.staff_id)
    else {
        return Err(invalid(
            "staff_unavailable",
            "this staff member does not perform these services",
        ));
    };
    if inputs.services.len() != request.service_ids.len() {
        return Err(invalid(
            "invalid",
            "service_ids: a service is not available",
        ));
    }
    let mut services = Vec::with_capacity(inputs.services.len());
    for (product_id, _, minutes, price) in &inputs.services {
        let duration_min = minutes
            .and_then(|m| u32::try_from(m).ok())
            .ok_or_else(|| invalid("invalid", "service_ids: a service has no duration"))?;
        let price_minor = Money::from_minor(*price)
            .map_err(|_| invalid("invalid", "service_ids: a service has no valid price"))?;
        services.push(AppointmentService {
            product_id: *product_id,
            duration_min,
            price_minor,
        });
    }
    let client_name = validate::name(&request.client_name, "client_name")
        .map_err(|_| invalid("invalid_name", "enter your name (up to 128 characters)"))?;
    let draft = AppointmentPayload {
        v: buzz_crm::space::PAYLOAD_VERSION,
        staff_id: request.staff_id,
        date: day.format("%Y-%m-%d").to_string(),
        start,
        services,
        customer_id,
        client_name: Some(client_name),
        client_phone: Some(phone),
        status: AppointmentStatus::Booked,
        note: request.note.clone(),
        source: AppointmentSource::Online,
        outside_hours: false,
    };
    // Round-trip through the appointment rules (note length, midnight, …).
    let raw = serde_json::to_string(&draft)
        .map_err(|e| internal_error(&format!("booking payload: {e}")).into_response())?;
    let payload =
        AppointmentPayload::parse(&raw).map_err(|e| invalid("invalid", &e.to_string()))?;
    Ok(Booking {
        payload,
        staff_name: staff_name.clone(),
        service_names: inputs.services.iter().map(|s| s.1.clone()).collect(),
    })
}

fn code_problem(check: CodeCheck) -> Response {
    match check {
        CodeCheck::Verified { .. } => {
            internal_error("verified code reported as a problem").into_response()
        }
        CodeCheck::Wrong { attempts_left } => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "wrong_code",
                "message": "the code is wrong",
                "attempts_left": attempts_left,
            })),
        )
            .into_response(),
        CodeCheck::Unknown | CodeCheck::Expired => public_error(
            StatusCode::GONE,
            "code_expired",
            "the code expired; ask for a new one",
        ),
        CodeCheck::Used => public_error(
            StatusCode::CONFLICT,
            "code_used",
            "this code already booked a visit; ask for a new one",
        ),
        CodeCheck::Locked => public_error(
            StatusCode::GONE,
            "code_locked",
            "too many wrong attempts; ask for a new code",
        ),
    }
}

/// `POST /public/booking/appointments` — book with a verified phone.
pub async fn book(
    State(state): State<Arc<AppState>>,
    extensions: Extensions,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let page = match open_page(&state, &headers, peer(&extensions), RateBucket::Book).await {
        Ok(page) => page,
        Err(response) => return response,
    };
    let request: OnlineBookingRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(e) => return public_error(StatusCode::BAD_REQUEST, "invalid", &e.to_string()),
    };
    if let Err(e) = request.check() {
        return public_error(StatusCode::BAD_REQUEST, "invalid", &e.to_string());
    }
    let Ok(day) = NaiveDate::parse_from_str(&request.date, "%Y-%m-%d") else {
        return public_error(
            StatusCode::BAD_REQUEST,
            "invalid",
            "date must be YYYY-MM-DD",
        );
    };
    let Ok(start) = parse_clock(&request.start) else {
        return public_error(StatusCode::BAD_REQUEST, "invalid", "start must be HH:MM");
    };

    let hash = match code_hash(
        &state,
        page.community(),
        request.verification_id,
        &request.code,
    ) {
        Ok(hash) => hash,
        Err(response) => return response,
    };
    let check = state
        .db
        .crm_check_booking_code(
            page.community(),
            request.verification_id,
            &hash,
            CODE_MAX_ATTEMPTS,
        )
        .await;
    let (phone, phone_key) = match check {
        Ok(CodeCheck::Verified { phone, phone_key }) => (phone, phone_key),
        Ok(problem) => return code_problem(problem),
        Err(e) => return internal_error(&format!("booking code check: {e}")).into_response(),
    };

    let inputs = match state
        .db
        .crm_slot_inputs(
            page.community(),
            page.space.channel_id,
            day,
            &request.service_ids,
            Some(request.staff_id),
        )
        .await
    {
        Ok(Some(inputs)) => inputs,
        Ok(None) => return unavailable(),
        Err(e) => return internal_error(&format!("booking inputs read: {e}")).into_response(),
    };
    let customer_id = match state
        .db
        .crm_customer_by_phone(page.community(), page.space.channel_id, &phone_key)
        .await
    {
        Ok(customer) => customer,
        Err(e) => return internal_error(&format!("booking customer read: {e}")).into_response(),
    };
    let booking = match plan_booking(&page, &request, &inputs, day, start, phone, customer_id) {
        Ok(booking) => booking,
        Err(response) => return response,
    };

    let appointment_id = Uuid::new_v4();
    let event = match sign_booking(
        &state,
        page.space.channel_id,
        appointment_id,
        &booking.payload,
    ) {
        Ok(event) => event,
        Err(response) => return response,
    };
    match ingest_online_booking(
        &page.tenant,
        &state,
        &event,
        page.space.channel_id,
        request.verification_id,
    )
    .await
    {
        Ok(()) => {}
        Err(OnlineBookingError::CodeSpent) => return code_problem(CodeCheck::Used),
        Err(OnlineBookingError::Ingest(IngestError::CanvasConflict(_))) => {
            return public_error(
                StatusCode::CONFLICT,
                "slot_taken",
                "this time was just booked; pick another",
            )
        }
        Err(OnlineBookingError::Ingest(IngestError::Rejected(message))) => {
            return public_error(StatusCode::BAD_REQUEST, "rejected", &message)
        }
        Err(OnlineBookingError::Ingest(other)) => {
            return internal_error(&format!("online booking: {other:?}")).into_response()
        }
    }
    let payload = &booking.payload;
    (
        StatusCode::CREATED,
        Json(json!({
            "appointment_id": appointment_id,
            "staff_name": booking.staff_name,
            "date": payload.date,
            "start": format_clock(payload.start),
            "end": format_clock(payload.end()),
            "services": booking.service_names,
            "total_minor": payload.total_minor(),
            "address": page.settings.address,
            "phone": page.settings.phone,
            "title": page.settings.title,
        })),
    )
        .into_response()
}

fn sign_booking(
    state: &AppState,
    channel_id: Uuid,
    appointment_id: Uuid,
    payload: &AppointmentPayload,
) -> Result<nostr::Event, Response> {
    let internal = |what: &str, e: &dyn std::fmt::Display| {
        internal_error(&format!("online booking {what}: {e}")).into_response()
    };
    let content = serde_json::to_string(payload).map_err(|e| internal("payload", &e))?;
    let tags = [
        Tag::parse(["h".to_owned(), channel_id.to_string()]),
        Tag::parse([tags::TAG_CRM_ID.to_owned(), appointment_id.to_string()]),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| internal("tags", &e))?;
    EventBuilder::new(Kind::Custom(KIND_CRM_ONLINE_BOOKING as u16), content)
        .tags(tags)
        .sign_with_keys(&state.relay_keypair)
        .map_err(|e| internal("signature", &e))
}

#[cfg(test)]
mod tests;
