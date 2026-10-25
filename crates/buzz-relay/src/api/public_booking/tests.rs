use super::*;
use buzz_db::crm::{PublicService, PublicStaff, SlotInputs};

fn settings(staff_ids: Vec<Uuid>, service_ids: Vec<Uuid>) -> OnlineBookingSettings {
    OnlineBookingSettings {
        v: 1,
        enabled: true,
        title: "Борода".into(),
        about: None,
        address: None,
        phone: None,
        horizon_days: 7,
        min_notice_min: 60,
        step_min: 15,
        staff_ids,
        service_ids,
    }
}

fn page(settings: OnlineBookingSettings) -> Page {
    Page {
        tenant: TenantContext::resolved(
            CommunityId::from_uuid(Uuid::from_u128(77)),
            "salon.example",
        ),
        space: CrmSpaceRecord {
            channel_id: Uuid::from_u128(1),
            settings: json!({"currency": "RUB", "timezone": "Europe/Moscow"}),
            revision_event_id: vec![0; 32],
            created_by: vec![0; 32],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        },
        settings,
    }
}

const OLGA: Uuid = Uuid::from_u128(10);
const IVAN: Uuid = Uuid::from_u128(11);
const CUT: Uuid = Uuid::from_u128(20);
const BEARD: Uuid = Uuid::from_u128(21);
const COLOR: Uuid = Uuid::from_u128(22);

fn service(id: Uuid, name: &str) -> PublicService {
    PublicService {
        id,
        name: name.into(),
        category: None,
        duration_min: 30,
        price_minor: 100_000,
    }
}

fn member(id: Uuid, name: &str, services: &[Uuid]) -> PublicStaff {
    PublicStaff {
        id,
        name: name.into(),
        position: None,
        service_ids: services.to_vec(),
    }
}

#[test]
fn the_page_offers_listed_services_that_someone_offered_performs() {
    let catalog = || {
        (
            vec![
                service(CUT, "Стрижка"),
                service(BEARD, "Борода"),
                service(COLOR, "Окрашивание"),
            ],
            vec![
                member(OLGA, "Ольга", &[CUT, COLOR]),
                member(IVAN, "Иван", &[BEARD]),
            ],
        )
    };
    let (services, staff) = catalog();
    let all = offer(&page(settings(vec![], vec![])), services, staff);
    assert_eq!(all["services"].as_array().unwrap().len(), 3);
    assert_eq!(all["staff"].as_array().unwrap().len(), 2);

    // Only Olga is offered: the beard trim nobody offered performs is hidden.
    let (services, staff) = catalog();
    let olga = offer(&page(settings(vec![OLGA], vec![])), services, staff);
    let names: Vec<_> = olga["services"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Стрижка", "Окрашивание"]);

    // Only the beard trim is offered: Olga has nothing to offer and is hidden.
    let (services, staff) = catalog();
    let beard = offer(&page(settings(vec![], vec![BEARD])), services, staff);
    assert_eq!(beard["staff"].as_array().unwrap().len(), 1);
    assert_eq!(beard["staff"][0]["service_ids"], json!([BEARD]));
}

#[test]
fn unlisted_staff_and_services_are_refused() {
    let listed = settings(vec![OLGA], vec![CUT]);
    assert_eq!(offer_problem(&listed, Some(OLGA), &[CUT]), None);
    assert_eq!(offer_problem(&listed, None, &[CUT]), None);
    assert!(offer_problem(&listed, Some(IVAN), &[CUT]).is_some());
    assert!(offer_problem(&listed, Some(OLGA), &[CUT, BEARD]).is_some());
}

fn inputs(now: &str) -> SlotInputs {
    SlotInputs {
        staff: vec![(OLGA, "Ольга".into(), json!([{"start": 600, "end": 1200}]))],
        busy: vec![],
        services: vec![(CUT, "Стрижка".into(), Some(45), 150_000)],
        now_local: now.parse().unwrap(),
    }
}

fn request(date: &str, start: &str) -> OnlineBookingRequest {
    OnlineBookingRequest {
        verification_id: Uuid::from_u128(5),
        code: "123456".into(),
        staff_id: OLGA,
        date: date.into(),
        start: start.into(),
        service_ids: vec![CUT],
        client_name: " Анна ".into(),
        note: Some("  ".into()),
    }
}

fn plan(now: &str, date: &str, start: &str) -> Result<Booking, StatusCode> {
    let request = request(date, start);
    plan_booking(
        &page(settings(vec![], vec![])),
        &request,
        &inputs(now),
        date.parse().unwrap(),
        parse_clock(start).unwrap(),
        "+7 900 000-00-01".into(),
        None,
    )
    .map_err(|response| response.status())
}

#[test]
fn bookings_are_priced_from_the_catalog_as_online_visits() {
    let booking = plan("2026-11-02T08:00:00", "2026-11-02", "10:00").unwrap();
    let payload = &booking.payload;
    assert_eq!(payload.source, AppointmentSource::Online);
    assert_eq!(payload.status, AppointmentStatus::Booked);
    assert_eq!(payload.services[0].duration_min, 45);
    assert_eq!(payload.total_minor(), 150_000);
    assert_eq!(payload.client_name.as_deref(), Some("Анна"));
    assert_eq!(payload.client_phone.as_deref(), Some("+7 900 000-00-01"));
    assert_eq!(payload.note, None);
    assert!(!payload.outside_hours);
    assert_eq!(booking.staff_name, "Ольга");
}

#[test]
fn bookings_respect_notice_horizon_and_grid() {
    // One hour of notice: 09:30 now allows 10:30, not 10:15.
    assert_eq!(
        plan("2026-11-02T09:30:00", "2026-11-02", "10:15").err(),
        Some(StatusCode::BAD_REQUEST)
    );
    assert!(plan("2026-11-02T09:30:00", "2026-11-02", "10:30").is_ok());
    // Seven days ahead is the last bookable day.
    assert!(plan("2026-11-02T09:30:00", "2026-11-09", "10:30").is_ok());
    assert!(plan("2026-11-02T09:30:00", "2026-11-10", "10:30").is_err());
    // Off the 15-minute grid.
    assert!(plan("2026-11-02T08:00:00", "2026-11-02", "10:05").is_err());
}

#[test]
fn staff_who_cannot_do_the_services_are_refused() {
    let mut nobody = inputs("2026-11-02T08:00:00");
    nobody.staff.clear();
    let request = request("2026-11-02", "10:00");
    let refused = plan_booking(
        &page(settings(vec![], vec![])),
        &request,
        &nobody,
        "2026-11-02".parse().unwrap(),
        600,
        "+7 900 000-00-01".into(),
        None,
    );
    assert!(refused.is_err());
}

#[test]
fn addresses_come_from_the_peer_unless_one_proxy_is_trusted() {
    let peer: SocketAddr = "10.0.0.5:4000".parse().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_static("1.1.1.1, 203.0.113.9"),
    );
    assert_eq!(client_ip(false, &headers, Some(peer)), peer.ip());
    assert_eq!(
        client_ip(true, &headers, Some(peer)),
        "203.0.113.9".parse::<IpAddr>().unwrap()
    );
    assert_eq!(client_ip(true, &HeaderMap::new(), Some(peer)), peer.ip());
}

#[test]
fn each_bucket_has_its_own_hourly_allowance() {
    let limiter = rate_limiter();
    let community = CommunityId::from_uuid(Uuid::from_u128(1));
    let ip = IpAddr::from([192, 0, 2, 1]);
    for _ in 0..RateBucket::Code.per_hour() {
        assert!(!rate_limited(&limiter, (community, ip, RateBucket::Code)));
    }
    assert!(rate_limited(&limiter, (community, ip, RateBucket::Code)));
    assert!(!rate_limited(&limiter, (community, ip, RateBucket::Book)));
    assert!(!rate_limited(
        &limiter,
        (community, IpAddr::from([192, 0, 2, 2]), RateBucket::Code)
    ));
}

#[test]
fn sms_text_carries_the_code_and_a_short_salon_name() {
    let text = sms_text(
        "Очень длинное название салона красоты и барбершопа",
        "012345",
    );
    assert!(text.starts_with("012345 — код"));
    assert!(text.chars().count() < 120);
}
