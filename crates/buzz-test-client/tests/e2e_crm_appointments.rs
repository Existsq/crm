//! End-to-end tests for the appointment journal (kind 42133,
//! `GET /crm/appointments`, `GET /crm/slots`).
//!
//! # Running
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 \
//! DATABASE_URL=postgres://buzz:buzz_dev@localhost:5432/buzz \
//!   cargo test -p buzz-test-client --test e2e_crm_appointments -- --ignored --test-threads=1
//! ```

mod crm_support;

use crm_support::*;
use nostr::Keys;
use serde_json::{json, Value};
use uuid::Uuid;

/// A day far enough ahead that every slot on it is still in the future.
fn booking_day() -> String {
    (chrono::Utc::now().date_naive() + chrono::Days::new(30))
        .format("%Y-%m-%d")
        .to_string()
}

fn service(name: &str, duration: u32, price: i64) -> Value {
    json!({
        "v": 1,
        "name": name,
        "unit": "piece",
        "price_minor": price,
        "vat": "none",
        "payment_object": "service",
        "duration_min": duration,
    })
}

fn booking(
    staff: Uuid,
    day: &str,
    start: &str,
    services: &[(Uuid, u32, i64)],
    status: &str,
) -> Value {
    json!({
        "v": 1,
        "staff_id": staff,
        "date": day,
        "start": start,
        "services": services
            .iter()
            .map(|(id, duration, price)| json!({"product_id": id, "duration_min": duration, "price_minor": price}))
            .collect::<Vec<_>>(),
        "client_name": "Анна",
        "client_phone": "+7 900 000-00-01",
        "status": status,
    })
}

async fn create(keys: &Keys, kind: u16, channel: Uuid, id: Uuid, payload: &Value) {
    accept(
        keys,
        &crm_event(keys, kind, channel, Some(id), payload, None),
    )
    .await;
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn appointments_respect_hours_overlaps_and_slots() {
    let pool = pool().await;
    let (owner, channel, community) = fresh_space(&pool).await;
    let member = Keys::generate();
    let guest = Keys::generate();
    add_member(&owner, channel, &member, "member").await;
    add_member(&owner, channel, &guest, "guest").await;
    let day = booking_day();

    let haircut = Uuid::new_v4();
    let beard = Uuid::new_v4();
    create(
        &owner,
        KIND_CRM_PRODUCT,
        channel,
        haircut,
        &service("Стрижка", 60, 150_000),
    )
    .await;
    create(
        &owner,
        KIND_CRM_PRODUCT,
        channel,
        beard,
        &service("Борода", 30, 80_000),
    )
    .await;
    let olga = Uuid::new_v4();
    let ivan = Uuid::new_v4();
    let staff = |name: &str, services: &[Uuid]| json!({"v": 1, "name": name, "service_ids": services, "archived": false});
    create(
        &owner,
        KIND_CRM_STAFF,
        channel,
        olga,
        &staff("Ольга", &[haircut, beard]),
    )
    .await;
    create(
        &owner,
        KIND_CRM_STAFF,
        channel,
        ivan,
        &staff("Иван", &[haircut]),
    )
    .await;
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_STAFF_SCHEDULE,
            channel,
            None,
            &json!({"v": 1, "staff_id": olga, "days": [{"date": day, "intervals": [
                {"start": "09:00", "end": "13:00"}, {"start": "14:00", "end": "20:00"}
            ]}]}),
            None,
        ),
    )
    .await;

    // Free windows before anything is booked; Ivan has no hours that day.
    let both = format!("/crm/slots?date={day}&service_ids={haircut},{beard}&step=30");
    let (status, body) = get(Some(&guest), &both).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["duration_min"], json!(90));
    assert_eq!(body["total_minor"], json!(230_000));
    assert_eq!(
        body["staff"].as_array().unwrap().len(),
        1,
        "only Olga does both: {body}"
    );
    let starts = body["staff"][0]["starts"].as_array().unwrap();
    assert_eq!(starts.first(), Some(&json!("09:00")));
    assert!(
        starts.contains(&json!("11:30")) && !starts.contains(&json!("12:00")),
        "{body}"
    );
    assert_eq!(starts.last(), Some(&json!("18:30")));
    let (_, body) = get(
        Some(&guest),
        &format!("/crm/slots?date={day}&service_ids={haircut}"),
    )
    .await;
    assert_eq!(body["staff"].as_array().unwrap().len(), 2, "{body}");

    // A member books 10:00–11:30; guests cannot book.
    let anna = Uuid::new_v4();
    let services = [(haircut, 60, 150_000), (beard, 30, 80_000)];
    reject(
        &guest,
        &crm_event(
            &guest,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(anna),
            &booking(olga, &day, "10:00", &services, "booked"),
            None,
        ),
        "restricted:",
    )
    .await;
    let first = crm_event(
        &member,
        KIND_CRM_APPOINTMENT,
        channel,
        Some(anna),
        &booking(olga, &day, "10:00", &services, "booked"),
        None,
    );
    accept(&member, &first).await;
    let (status, body) = get(
        Some(&guest),
        &format!("/crm/appointments?from={day}&to={day}"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let list = body["appointments"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{body}");
    assert_eq!(list[0]["start"], json!(600));
    assert_eq!(list[0]["end"], json!(690));
    assert_eq!(list[0]["total_minor"], json!(230_000));
    assert_eq!(list[0]["services"][0]["name"], json!("Стрижка"));
    assert_eq!(list[0]["revision"], json!(first.id.to_hex()));

    // Overlaps, hours and services are checked at the relay.
    let overlap = reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(Uuid::new_v4()),
            &booking(olga, &day, "11:00", &services[..1], "booked"),
            None,
        ),
        "conflict:",
    )
    .await;
    assert!(overlap.contains("10:00–11:30"), "{overlap}");
    reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(Uuid::new_v4()),
            &booking(olga, &day, "12:30", &services[..1], "booked"),
            None,
        ),
        "invalid:",
    )
    .await;
    reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(Uuid::new_v4()),
            &booking(ivan, &day, "10:00", &services, "booked"),
            None,
        ),
        "invalid:",
    )
    .await;
    // Outside hours is allowed when asked for explicitly.
    let mut late = booking(olga, &day, "20:00", &services[..1], "booked");
    late["outside_hours"] = json!(true);
    accept(
        &member,
        &crm_event(
            &member,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(Uuid::new_v4()),
            &late,
            None,
        ),
    )
    .await;

    // The booked window is gone from the slots.
    let (_, body) = get(Some(&guest), &both).await;
    let starts = body["staff"][0]["starts"].as_array().unwrap();
    assert!(
        !starts.contains(&json!("10:00")) && !starts.contains(&json!("09:00")),
        "{body}"
    );
    assert!(starts.contains(&json!("11:30")), "{body}");

    // Cancelling needs the revision and frees the time.
    reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(anna),
            &booking(olga, &day, "10:00", &services, "cancelled"),
            Some(&"0".repeat(64)),
        ),
        "conflict:",
    )
    .await;
    accept(
        &member,
        &crm_event(
            &member,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(anna),
            &booking(olga, &day, "10:00", &services, "cancelled"),
            Some(&first.id.to_hex()),
        ),
    )
    .await;
    let (_, body) = get(Some(&guest), &both).await;
    assert!(
        body["staff"][0]["starts"]
            .as_array()
            .unwrap()
            .contains(&json!("10:00")),
        "{body}"
    );
    let (status, body) = get(Some(&guest), &format!("/crm/appointments/{anna}")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], json!("cancelled"));

    // Visit analytics: one visit holding time, one cancelled.
    let (status, body) = get(
        Some(&guest),
        &format!("/crm/analytics/services?from={day}&to={day}"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let olga_stats = body["staff"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["staff_id"] == json!(olga))
        .cloned()
        .expect("olga in staff analytics");
    assert_eq!(olga_stats["visits"], json!(1), "{body}");
    assert_eq!(olga_stats["cancelled"], json!(1), "{body}");
    assert_eq!(olga_stats["worked_min"], json!(600), "{body}");
    assert_eq!(body["services"][0]["product_id"], json!(haircut), "{body}");

    // Reads are bounded and members-only.
    let (status, _) = get(Some(&guest), "/crm/appointments").await;
    assert_eq!(status, 400);
    let (status, _) = get(
        Some(&guest),
        &format!("/crm/slots?date={day}&service_ids={haircut}&step=7"),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = get(
        Some(&Keys::generate()),
        &format!("/crm/appointments?from={day}&to={day}"),
    )
    .await;
    assert!(status == 403 || status == 404, "{status}");

    cleanup_space(&pool, community).await;
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn a_sale_pays_for_a_visit_and_its_void_unpays_it() {
    let pool = pool().await;
    let (owner, channel, community) = fresh_space(&pool).await;
    let warehouse: Uuid = get_space(&owner).await["warehouses"][0]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("default warehouse");
    let day = booking_day();
    let haircut = Uuid::new_v4();
    create(
        &owner,
        KIND_CRM_PRODUCT,
        channel,
        haircut,
        &service("Стрижка", 60, 150_000),
    )
    .await;
    let olga = Uuid::new_v4();
    create(
        &owner,
        KIND_CRM_STAFF,
        channel,
        olga,
        &json!({"v": 1, "name": "Ольга", "service_ids": [haircut], "archived": false}),
    )
    .await;
    let visit = Uuid::new_v4();
    let booked = booking(olga, &day, "10:00", &[(haircut, 60, 150_000)], "arrived");
    let mut late = booked.clone();
    late["outside_hours"] = json!(true);
    create(&owner, KIND_CRM_APPOINTMENT, channel, visit, &late).await;
    let cancelled = Uuid::new_v4();
    let mut gone = booking(olga, &day, "15:00", &[(haircut, 60, 150_000)], "cancelled");
    gone["outside_hours"] = json!(true);
    create(&owner, KIND_CRM_APPOINTMENT, channel, cancelled, &gone).await;

    let sale = |appointment: Uuid| {
        json!({
            "v": 1,
            "warehouse_id": warehouse,
            "payment": "card",
            "lines": [{"product_id": haircut, "qty_milli": 1000, "price_minor": 150_000}],
            "appointment_id": appointment,
        })
    };
    // Unknown and cancelled visits cannot be paid; nothing is sold.
    for appointment in [Uuid::new_v4(), cancelled] {
        reject(
            &owner,
            &crm_event(
                &owner,
                KIND_CRM_SALE,
                channel,
                Some(Uuid::new_v4()),
                &sale(appointment),
                None,
            ),
            "invalid: appointment_id",
        )
        .await;
    }
    let (_, body) = get(Some(&owner), "/crm/sales").await;
    assert_eq!(body["sales"], json!([]), "{body}");

    let paid = Uuid::new_v4();
    let sale_event = crm_event(
        &owner,
        KIND_CRM_SALE,
        channel,
        Some(paid),
        &sale(visit),
        None,
    );
    accept(&owner, &sale_event).await;
    let (_, body) = get(Some(&owner), &format!("/crm/appointments/{visit}")).await;
    assert_eq!(body["status"], json!("done"), "{body}");
    assert_eq!(body["sale_id"], json!(paid));
    assert_eq!(body["revision"], json!(sale_event.id.to_hex()));

    // One payment per visit, and a paid visit keeps its time.
    reject(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_SALE,
            channel,
            Some(Uuid::new_v4()),
            &sale(visit),
            None,
        ),
        "conflict:",
    )
    .await;
    let mut moved = late.clone();
    moved["start"] = json!("11:00");
    reject(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(visit),
            &moved,
            Some(&sale_event.id.to_hex()),
        ),
        "invalid: the visit is paid",
    )
    .await;

    // Voiding the sale unpays the visit.
    let void = sign(
        &owner,
        KIND_CRM_SALE_VOID,
        &json!({"v": 1, "reason": "ошибка"}).to_string(),
        vec![
            tag(&["h", &channel.to_string()]),
            tag(&["crm-id", &paid.to_string()]),
        ],
    );
    accept(&owner, &void).await;
    let (_, body) = get(Some(&owner), &format!("/crm/appointments/{visit}")).await;
    assert_eq!(body["status"], json!("arrived"), "{body}");
    assert_eq!(body["sale_id"], Value::Null);
    assert_eq!(body["revision"], json!(void.id.to_hex()));
    let (_, body) = get(
        Some(&owner),
        &format!("/crm/appointments?customer_id={}", Uuid::new_v4()),
    )
    .await;
    assert_eq!(body["appointments"], json!([]));

    cleanup_space(&pool, community).await;
}
