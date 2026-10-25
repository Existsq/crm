//! End-to-end tests for CRM staff and working days (kinds 42131, 42132,
//! `GET /crm/staff`, `GET /crm/schedule`) and service durations.
//!
//! # Running
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 \
//! DATABASE_URL=postgres://buzz:buzz_dev@localhost:5432/buzz \
//!   cargo test -p buzz-test-client --test e2e_crm_staff -- --ignored --test-threads=1
//! ```

mod crm_support;

use crm_support::*;
use nostr::Keys;
use serde_json::{json, Value};
use uuid::Uuid;

fn crm_event(
    keys: &Keys,
    kind: u16,
    channel: Uuid,
    id: Option<Uuid>,
    payload: &Value,
    expected_revision: Option<&str>,
) -> nostr::Event {
    let mut tags = vec![tag(&["h", &channel.to_string()])];
    if let Some(id) = id {
        tags.push(tag(&["crm-id", &id.to_string()]));
    }
    if let Some(revision) = expected_revision {
        tags.push(tag(&["expected-revision", revision]));
    }
    sign(keys, kind, &payload.to_string(), tags)
}

fn service(name: &str, payment_object: &str, duration: Option<u32>) -> Value {
    let mut value = json!({
        "v": 1,
        "name": name,
        "unit": "piece",
        "price_minor": 150_000,
        "vat": "none",
        "payment_object": payment_object,
    });
    if let Some(duration) = duration {
        value["duration_min"] = json!(duration);
    }
    value
}

fn staff(name: &str, services: &[Uuid], archived: bool) -> Value {
    json!({
        "v": 1,
        "name": name,
        "position": "Барбер",
        "service_ids": services,
        "archived": archived,
    })
}

fn schedule(staff_id: Uuid, days: Value) -> Value {
    json!({ "v": 1, "staff_id": staff_id, "days": days })
}

async fn get(keys: Option<&Keys>, path_and_query: &str) -> (u16, Value) {
    let mut request = http().get(format!("{}{path_and_query}", relay_http_url()));
    if let Some(keys) = keys {
        request = request.header("X-Pubkey", keys.public_key().to_hex());
    }
    let resp = request.send().await.expect("GET");
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

async fn accept(keys: &Keys, event: &nostr::Event) {
    let (status, accepted, message) = submit(keys, event).await;
    assert!(
        status == 200 && accepted && message.is_empty(),
        "{status} {message}"
    );
}

async fn reject(keys: &Keys, event: &nostr::Event, prefix: &str) {
    let (status, accepted, message) = submit(keys, event).await;
    assert!(
        !accepted && message.starts_with(prefix),
        "expected {prefix}, got {status} {message}"
    );
}

async fn space(pool: &sqlx::PgPool) -> (Keys, Uuid, Uuid) {
    let owner = Keys::generate();
    let channel = create_channel(&owner, "private").await;
    let community = community_of(pool, channel).await;
    cleanup_space(pool, community).await;
    grant_community_role(pool, community, &owner, "admin").await;
    accept(&owner, &space_create(&owner, channel, Uuid::new_v4())).await;
    (owner, channel, community)
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn services_carry_durations_and_list_separately() {
    let pool = pool().await;
    let (owner, channel, community) = space(&pool).await;

    let haircut = Uuid::new_v4();
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PRODUCT,
            channel,
            Some(haircut),
            &service("Стрижка", "service", Some(45)),
            None,
        ),
    )
    .await;
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PRODUCT,
            channel,
            Some(Uuid::new_v4()),
            &service("Шампунь", "commodity", None),
            None,
        ),
    )
    .await;
    // Durations are bookable-only and on a 5-minute grid.
    reject(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PRODUCT,
            channel,
            Some(Uuid::new_v4()),
            &service("Воск", "commodity", Some(30)),
            None,
        ),
        "invalid:",
    )
    .await;
    reject(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PRODUCT,
            channel,
            Some(Uuid::new_v4()),
            &service("Бритьё", "service", Some(32)),
            None,
        ),
        "invalid:",
    )
    .await;

    let (status, body) = get(Some(&owner), "/crm/products?kind=services").await;
    assert_eq!(status, 200, "{body}");
    let products = body["products"].as_array().unwrap();
    assert_eq!(products.len(), 1, "{body}");
    assert_eq!(products[0]["name"], json!("Стрижка"));
    assert_eq!(products[0]["duration_min"], json!(45));
    let (_, body) = get(Some(&owner), "/crm/products?kind=goods").await;
    assert_eq!(body["products"].as_array().unwrap().len(), 1, "{body}");

    cleanup_space(&pool, community).await;
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn staff_are_created_updated_and_scheduled() {
    let pool = pool().await;
    let (owner, channel, community) = space(&pool).await;
    let member = Keys::generate();
    let guest = Keys::generate();
    add_member(&owner, channel, &member, "member").await;
    add_member(&owner, channel, &guest, "guest").await;

    let haircut = Uuid::new_v4();
    let shampoo = Uuid::new_v4();
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PRODUCT,
            channel,
            Some(haircut),
            &service("Стрижка", "service", Some(60)),
            None,
        ),
    )
    .await;
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PRODUCT,
            channel,
            Some(shampoo),
            &service("Шампунь", "commodity", None),
            None,
        ),
    )
    .await;

    // A member adds a master; goods are not services.
    let olga = Uuid::new_v4();
    reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_STAFF,
            channel,
            Some(olga),
            &staff("Ольга", &[shampoo], false),
            None,
        ),
        "invalid:",
    )
    .await;
    let create = crm_event(
        &member,
        KIND_CRM_STAFF,
        channel,
        Some(olga),
        &staff(" Ольга ", &[haircut], false),
        None,
    );
    accept(&member, &create).await;
    let (status, body) = get(Some(&member), &format!("/crm/staff/{olga}")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["name"], json!("Ольга"));
    assert_eq!(body["service_ids"], json!([haircut]));
    assert_eq!(body["revision"], json!(create.id.to_hex()));

    // Guests cannot edit staff.
    reject(
        &guest,
        &crm_event(
            &guest,
            KIND_CRM_STAFF,
            channel,
            Some(Uuid::new_v4()),
            &staff("Гость", &[], false),
            None,
        ),
        "restricted:",
    )
    .await;

    // Updates need the current revision.
    reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_STAFF,
            channel,
            Some(olga),
            &staff("Ольга К.", &[], false),
            Some(&"0".repeat(64)),
        ),
        "conflict:",
    )
    .await;
    let update = crm_event(
        &member,
        KIND_CRM_STAFF,
        channel,
        Some(olga),
        &staff("Ольга К.", &[haircut], false),
        Some(&create.id.to_hex()),
    );
    accept(&member, &update).await;

    // Working days: set two, then change one; unlisted days stay.
    accept(
        &member,
        &crm_event(
            &member,
            KIND_CRM_STAFF_SCHEDULE,
            channel,
            None,
            &schedule(
                olga,
                json!([
                    {"date": "2026-11-02", "intervals": [{"start": "09:00", "end": "13:00"}, {"start": "14:00", "end": "20:00"}]},
                    {"date": "2026-11-03", "intervals": [{"start": "10:00", "end": "18:00"}]}
                ]),
            ),
            None,
        ),
    )
    .await;
    accept(
        &member,
        &crm_event(
            &member,
            KIND_CRM_STAFF_SCHEDULE,
            channel,
            None,
            &schedule(olga, json!([{"date": "2026-11-03", "intervals": []}])),
            None,
        ),
    )
    .await;
    // Overlapping intervals are invalid.
    reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_STAFF_SCHEDULE,
            channel,
            None,
            &schedule(
                olga,
                json!([{"date": "2026-11-04", "intervals": [{"start": "09:00", "end": "13:00"}, {"start": "12:00", "end": "15:00"}]}]),
            ),
            None,
        ),
        "invalid:",
    )
    .await;
    let (status, body) = get(Some(&guest), "/crm/schedule?from=2026-11-01&to=2026-11-30").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["days"],
        json!([
            {"staff_id": olga, "date": "2026-11-02", "intervals": [{"start": 540, "end": 780}, {"start": 840, "end": 1200}]},
            {"staff_id": olga, "date": "2026-11-03", "intervals": []}
        ])
    );
    let (status, _) = get(Some(&guest), "/crm/schedule?from=2026-11-01&to=2027-03-01").await;
    assert_eq!(status, 400);

    // Archive: gone from the default list, no schedule changes.
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_STAFF,
            channel,
            Some(olga),
            &staff("Ольга К.", &[haircut], true),
            Some(&update.id.to_hex()),
        ),
    )
    .await;
    let (_, body) = get(Some(&member), "/crm/staff").await;
    assert_eq!(body["staff"], json!([]));
    let (_, body) = get(Some(&member), "/crm/staff?archived=true").await;
    assert_eq!(body["staff"].as_array().unwrap().len(), 1, "{body}");
    reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_STAFF_SCHEDULE,
            channel,
            None,
            &schedule(olga, json!([{"date": "2026-11-05", "intervals": []}])),
            None,
        ),
        "invalid:",
    )
    .await;

    // Outsiders cannot read staff.
    let (status, _) = get(Some(&Keys::generate()), "/crm/staff").await;
    assert!(status == 403 || status == 404, "{status}");

    cleanup_space(&pool, community).await;
}
