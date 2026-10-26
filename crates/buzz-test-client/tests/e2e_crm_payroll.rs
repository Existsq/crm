//! End-to-end tests for staff payroll (kinds 42160–42162, `GET /crm/payroll`).
//!
//! # Running
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 \
//! DATABASE_URL=postgres://buzz:buzz_dev@localhost:5432/buzz \
//!   cargo test -p buzz-test-client --test e2e_crm_payroll -- --ignored --test-threads=1
//! ```

mod crm_support;

use crm_support::*;
use nostr::Keys;
use serde_json::{json, Value};
use uuid::Uuid;

/// A day safely in the past in any timezone, so its working hours count.
fn work_day() -> String {
    (chrono::Utc::now().date_naive() - chrono::Days::new(3))
        .format("%Y-%m-%d")
        .to_string()
}

fn staff_row(body: &Value, staff: Uuid) -> Value {
    body["staff"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["staff_id"] == json!(staff))
        .cloned()
        .unwrap_or_else(|| panic!("{staff} missing from {body}"))
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn payroll_accrues_from_done_visits_and_shifts_minus_payouts() {
    let pool = pool().await;
    let (owner, channel, community) = fresh_space(&pool).await;
    let member = Keys::generate();
    add_member(&owner, channel, &member, "member").await;
    let day = work_day();

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
            &json!({"v": 1, "name": "Ольга", "service_ids": [haircut], "archived": false}),
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
            &json!({"v": 1, "staff_id": olga, "days": [{"date": day, "intervals": [
                {"start": "10:00", "end": "19:00"}
            ]}]}),
            None,
        ),
    )
    .await;
    for (start, status) in [("10:00", "done"), ("11:00", "done"), ("12:00", "no_show")] {
        accept(
            &owner,
            &crm_event(
                &owner,
                KIND_CRM_APPOINTMENT,
                channel,
                Some(Uuid::new_v4()),
                &json!({"v": 1, "staff_id": olga, "date": day, "start": start,
                        "services": [{"product_id": haircut, "duration_min": 60, "price_minor": 150_000}],
                        "client_name": "Анна", "status": status}),
                None,
            ),
        )
        .await;
    }

    // Rates: 40 % of services, 100 ₽ per visit, 500 ₽ per working day.
    let rates = json!({"v": 1, "staff_id": olga, "percent_bp": 4000,
                       "per_visit_minor": 10_000, "shift_minor": 50_000});
    reject(
        &member,
        &crm_event(&member, KIND_CRM_PAY_RATES, channel, None, &rates, None),
        "restricted:",
    )
    .await;
    accept(
        &owner,
        &crm_event(&owner, KIND_CRM_PAY_RATES, channel, None, &rates, None),
    )
    .await;
    reject(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PAY_RATES,
            channel,
            None,
            &json!({"v": 1, "staff_id": Uuid::new_v4(), "percent_bp": 100}),
            None,
        ),
        "invalid:",
    )
    .await;

    let advance = Uuid::new_v4();
    let payout =
        json!({"v": 1, "staff_id": olga, "amount_minor": 100_000, "date": day, "note": "аванс"});
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PAYOUT,
            channel,
            Some(advance),
            &payout,
            None,
        ),
    )
    .await;
    reject(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PAYOUT,
            channel,
            Some(advance),
            &json!({"v": 1, "staff_id": olga, "amount_minor": 100_001, "date": day}),
            None,
        ),
        "conflict:",
    )
    .await;
    let mistake = Uuid::new_v4();
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PAYOUT,
            channel,
            Some(mistake),
            &json!({"v": 1, "staff_id": olga, "amount_minor": 5_000, "date": day}),
            None,
        ),
    )
    .await;
    let void = json!({"v": 1, "payout_id": mistake});
    accept(
        &owner,
        &crm_event(&owner, KIND_CRM_PAYOUT_VOID, channel, None, &void, None),
    )
    .await;
    // A second signed cancellation (new timestamp, so not a duplicate event).
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    reject(
        &owner,
        &crm_event(&owner, KIND_CRM_PAYOUT_VOID, channel, None, &void, None),
        "conflict:",
    )
    .await;

    let path = format!("/crm/payroll?from={day}&to={day}");
    let (status, _) = get(Some(&member), &path).await;
    assert_eq!(status, 403, "members do not see payroll");
    let (status, body) = get(Some(&owner), &path).await;
    assert_eq!(status, 200, "{body}");
    let row = staff_row(&body, olga);
    assert_eq!(row["visits"], json!(2), "{body}");
    assert_eq!(row["revenue_minor"], json!(300_000), "{body}");
    assert_eq!(row["shifts"], json!(1), "{body}");
    // 40 % × 3 000 ₽ + 2 × 100 ₽ + 500 ₽ = 1 900 ₽; paid 1 000 ₽.
    assert_eq!(row["accrued_minor"], json!(190_000), "{body}");
    assert_eq!(row["paid_minor"], json!(100_000), "{body}");
    assert_eq!(row["balance_minor"], json!(90_000), "{body}");
    assert_eq!(body["payouts"].as_array().unwrap().len(), 2, "{body}");

    cleanup_space(&pool, community).await;
}
