//! End-to-end tests for public online booking (kinds 42134–42135,
//! `GET /crm/online-booking`, `/book`, `/public/booking/*`).
//!
//! # Running
//!
//! The relay must return codes to the test (`BUZZ_BOOKING_EXPOSE_CODES=true`)
//! and log SMS instead of sending them (`BUZZ_BOOKING_SMS=log`, the default).
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 \
//! DATABASE_URL=postgres://buzz:buzz_dev@localhost:5432/buzz \
//!   cargo test -p buzz-test-client --test e2e_crm_online_booking -- --ignored --test-threads=1
//! ```

mod crm_support;

use crm_support::*;
use nostr::Keys;
use serde_json::{json, Value};
use uuid::Uuid;

fn booking_day() -> String {
    (chrono::Utc::now().date_naive() + chrono::Days::new(20))
        .format("%Y-%m-%d")
        .to_string()
}

async fn public_get(path: &str) -> (u16, Value) {
    get(None, path).await
}

async fn public_post(path: &str, body: &Value) -> (u16, Value) {
    let resp = http()
        .post(format!("{}{path}", relay_http_url()))
        .json(body)
        .send()
        .await
        .expect("POST");
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// Ask for a code; returns `(verification_id, code)`.
async fn code_for(phone: &str) -> (String, String) {
    let (status, body) = public_post("/public/booking/code", &json!({ "phone": phone })).await;
    assert_eq!(status, 200, "{body}");
    let code = body["dev_code"]
        .as_str()
        .expect("run the relay with BUZZ_BOOKING_EXPOSE_CODES=true")
        .to_owned();
    (body["verification_id"].as_str().unwrap().to_owned(), code)
}

fn settings(enabled: bool) -> Value {
    json!({
        "v": 1,
        "enabled": enabled,
        "title": "Барбершоп «Борода»",
        "address": "ул. Ленина, 1",
        "horizon_days": 60,
        "min_notice_min": 60,
        "step_min": 30,
    })
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn clients_book_online_with_a_phone_code() {
    let pool = pool().await;
    let (owner, channel, _community) = fresh_space(&pool).await;
    let member = Keys::generate();
    add_member(&owner, channel, &member, "member").await;
    let day = booking_day();

    let haircut = Uuid::new_v4();
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PRODUCT,
            channel,
            Some(haircut),
            &json!({"v": 1, "name": "Стрижка", "unit": "piece", "price_minor": 150_000,
                    "vat": "none", "payment_object": "service", "duration_min": 60}),
            None,
        ),
    )
    .await;
    let olga = Uuid::new_v4();
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_STAFF,
            channel,
            Some(olga),
            &json!({"v": 1, "name": "Ольга", "position": "Барбер", "service_ids": [haircut], "archived": false}),
            None,
        ),
    )
    .await;
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_STAFF_SCHEDULE,
            channel,
            None,
            &json!({"v": 1, "staff_id": olga, "days": [{"date": day, "intervals": [{"start": "10:00", "end": "14:00"}]}]}),
            None,
        ),
    )
    .await;

    // No settings yet: the page is closed and the staff read says so.
    let (status, body) = public_get("/public/booking").await;
    assert_eq!((status, body["error"].as_str()), (404, Some("unavailable")));
    let (status, body) = get(Some(&owner), "/crm/online-booking").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["settings"], Value::Null);
    assert_eq!(body["public_path"], json!("/book"));

    // Only managers configure the page; creates and updates use revisions.
    reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_ONLINE_BOOKING_SETTINGS,
            channel,
            None,
            &settings(true),
            None,
        ),
        "restricted:",
    )
    .await;
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_ONLINE_BOOKING_SETTINGS,
            channel,
            None,
            &settings(true),
            None,
        ),
    )
    .await;
    let mut second = settings(true);
    second["title"] = json!("Другой салон");
    reject(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_ONLINE_BOOKING_SETTINGS,
            channel,
            None,
            &second,
            None,
        ),
        "conflict:",
    )
    .await;
    let (_, saved) = get(Some(&owner), "/crm/online-booking").await;
    assert_eq!(saved["settings"]["title"], json!("Барбершоп «Борода»"));
    let revision = saved["revision"].as_str().unwrap().to_owned();

    // The page and its data need no account.
    let resp = http()
        .get(format!("{}/book", relay_http_url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert!(resp
        .headers()
        .get("content-security-policy")
        .is_some_and(|v| v.to_str().unwrap().contains("default-src 'none'")));
    let (status, info) = public_get("/public/booking").await;
    assert_eq!(status, 200, "{info}");
    assert_eq!(info["title"], json!("Барбершоп «Борода»"));
    assert_eq!(info["services"][0]["id"], json!(haircut));
    assert_eq!(info["staff"][0]["name"], json!("Ольга"));
    assert_eq!(info["step_min"], json!(30));

    let (status, slots) = public_get(&format!(
        "/public/booking/slots?date={day}&service_ids={haircut}"
    ))
    .await;
    assert_eq!(status, 200, "{slots}");
    assert_eq!(
        slots["staff"][0]["starts"],
        json!(["10:00", "10:30", "11:00", "11:30", "12:00", "12:30", "13:00"])
    );

    // Clients cannot sign online bookings themselves.
    reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_ONLINE_BOOKING,
            channel,
            Some(Uuid::new_v4()),
            &json!({"v": 1}),
            None,
        ),
        "restricted:",
    )
    .await;

    // A wrong code costs an attempt; the right one books once.
    let phone = format!("+7 9{:09}", rand_digits());
    let (verification, code) = code_for(&phone).await;
    let (status, body) = public_post("/public/booking/code", &json!({ "phone": phone })).await;
    assert_eq!((status, body["error"].as_str()), (429, Some("retry_later")));
    let request = |verification: &str, code: &str, start: &str| {
        json!({
            "verification_id": verification, "code": code, "staff_id": olga, "date": day,
            "start": start, "service_ids": [haircut], "client_name": "Анна", "note": "Покороче"
        })
    };
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let (status, body) = public_post(
        "/public/booking/appointments",
        &request(&verification, wrong, "10:00"),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (400, Some("wrong_code")),
        "{body}"
    );
    assert_eq!(body["attempts_left"], json!(4));
    let (status, booked) = public_post(
        "/public/booking/appointments",
        &request(&verification, &code, "10:00"),
    )
    .await;
    assert_eq!(status, 201, "{booked}");
    assert_eq!(booked["start"], json!("10:00"));
    assert_eq!(booked["end"], json!("11:00"));
    assert_eq!(booked["total_minor"], json!(150_000));
    let (status, body) = public_post(
        "/public/booking/appointments",
        &request(&verification, &code, "12:00"),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (409, Some("code_used")),
        "{body}"
    );

    // The salon sees it in the journal as an online visit.
    let (_, journal) = get(
        Some(&member),
        &format!("/crm/appointments?from={day}&to={day}"),
    )
    .await;
    let visit = &journal["appointments"][0];
    assert_eq!(visit["source"], json!("online"));
    assert_eq!(visit["status"], json!("booked"));
    assert_eq!(visit["client_name"], json!("Анна"));
    assert_eq!(visit["client_phone"], json!(phone));
    assert_eq!(visit["note"], json!("Покороче"));
    assert_eq!(visit["customer_id"], Value::Null);

    // A taken slot keeps the code for another try; a known phone links the card.
    let regular = format!("+7 9{:09}", rand_digits());
    let card = Uuid::new_v4();
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PARTY,
            channel,
            Some(card),
            &json!({"v": 1, "type": "customer", "name": "Пётр", "phone": regular.replacen("+7", "8", 1)}),
            None,
        ),
    )
    .await;
    let (verification, code) = code_for(&regular).await;
    let (status, body) = public_post(
        "/public/booking/appointments",
        &request(&verification, &code, "10:30"),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (409, Some("slot_taken")),
        "{body}"
    );
    let (status, body) = public_post(
        "/public/booking/appointments",
        &request(&verification, &code, "11:00"),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let (_, journal) = get(
        Some(&member),
        &format!("/crm/appointments?from={day}&to={day}"),
    )
    .await;
    assert_eq!(journal["appointments"][1]["customer_id"], json!(card));

    // Off-grid and outside-hours starts are refused before any write.
    let (verification, code) = code_for(&format!("+7 9{:09}", rand_digits())).await;
    let (status, body) = public_post(
        "/public/booking/appointments",
        &request(&verification, &code, "12:15"),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    let (status, body) = public_post(
        "/public/booking/appointments",
        &request(&verification, &code, "13:30"),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (400, Some("rejected")),
        "{body}"
    );

    // Turning the page off closes every public route.
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_ONLINE_BOOKING_SETTINGS,
            channel,
            None,
            &settings(false),
            Some(&revision),
        ),
    )
    .await;
    let (status, _) = public_get("/public/booking").await;
    assert_eq!(status, 404);
    let (status, _) = public_post("/public/booking/code", &json!({"phone": phone})).await;
    assert_eq!(status, 404);
}

fn rand_digits() -> u64 {
    u64::from(Uuid::new_v4().as_u128() as u32) % 1_000_000_000
}
