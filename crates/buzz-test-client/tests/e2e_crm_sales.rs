//! End-to-end tests for customers (kind 42103), sales (42104), sale voids
//! (42105) and the `GET /crm/parties`, `/crm/sales`, `/crm/analytics` reads.
//!
//! # Running
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 \
//! DATABASE_URL=postgres://buzz:buzz_dev@localhost:5432/buzz \
//!   cargo test -p buzz-test-client --test e2e_crm_sales -- --ignored --test-threads=1
//! ```

mod crm_support;

use crm_support::*;
use nostr::Keys;
use serde_json::{json, Value};
use uuid::Uuid;

fn entity_event(
    keys: &Keys,
    kind: u16,
    channel: Uuid,
    id: Uuid,
    payload: &Value,
    expected_revision: Option<&str>,
) -> nostr::Event {
    let mut tags = vec![
        tag(&["h", &channel.to_string()]),
        tag(&["crm-id", &id.to_string()]),
    ];
    if let Some(revision) = expected_revision {
        tags.push(tag(&["expected-revision", revision]));
    }
    sign(keys, kind, &payload.to_string(), tags)
}

async fn get(keys: &Keys, path_and_query: &str) -> (u16, Value) {
    let resp = http()
        .get(format!("{}{path_and_query}", relay_http_url()))
        .header("X-Pubkey", keys.public_key().to_hex())
        .send()
        .await
        .expect("GET");
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

async fn reject(keys: &Keys, event: &nostr::Event, status: u16, prefix: &str) -> String {
    let (got, accepted, message) = submit(keys, event).await;
    assert!(
        got == status && !accepted && message.starts_with(prefix),
        "expected {status} {prefix}…, got {got} {message}"
    );
    message
}

/// Owner (community admin) with a fresh CRM space; returns the default
/// warehouse id too.
async fn space(pool: &sqlx::PgPool) -> (Keys, Uuid, Uuid) {
    let owner = Keys::generate();
    let channel = create_channel(&owner, "private").await;
    let community = community_of(pool, channel).await;
    cleanup_space(pool, community).await;
    grant_community_role(pool, community, &owner, "admin").await;
    let warehouse = Uuid::new_v4();
    accept(&owner, &space_create(&owner, channel, warehouse)).await;
    (owner, channel, warehouse)
}

async fn product(keys: &Keys, channel: Uuid, name: &str, unit: &str) -> Uuid {
    let id = Uuid::new_v4();
    let payload = json!({
        "v": 1, "name": name, "unit": unit, "price_minor": 9_000,
        "vat": "vat10", "payment_object": "commodity"
    });
    accept(
        keys,
        &entity_event(keys, KIND_CRM_PRODUCT, channel, id, &payload, None),
    )
    .await;
    id
}

fn party(party_type: &str, name: &str) -> Value {
    json!({"v": 1, "type": party_type, "name": name, "phone": "+7 912 000-11-22"})
}

fn sale(warehouse: Uuid, customer: Option<Uuid>, lines: Value) -> Value {
    let mut payload = json!({"v": 1, "warehouse_id": warehouse, "payment": "card", "lines": lines});
    if let Some(customer) = customer {
        payload["customer_id"] = json!(customer);
    }
    payload
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn parties_are_created_searched_updated_and_kept_unique() {
    let pool = pool().await;
    let (owner, channel, _) = space(&pool).await;
    let member = Keys::generate();
    let guest = Keys::generate();
    add_member(&owner, channel, &member, "member").await;
    add_member(&owner, channel, &guest, "guest").await;

    let maria = Uuid::new_v4();
    let mut payload = party("customer", " Мария Иванова ");
    payload["inn"] = json!("500100732259");
    let create = entity_event(&member, KIND_CRM_PARTY, channel, maria, &payload, None);
    accept(&member, &create).await;
    let (status, body) = get(&guest, &format!("/crm/parties/{maria}")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["name"], json!("Мария Иванова"));
    assert_eq!(body["type"], json!("customer"));
    assert_eq!(body["revision"], json!(create.id.to_hex()));
    assert_eq!(body["purchases"]["sales_count"], json!(0));

    // Guests read but cannot write; INNs stay unique; CAS is enforced.
    let other = Uuid::new_v4();
    reject(
        &guest,
        &entity_event(
            &guest,
            KIND_CRM_PARTY,
            channel,
            other,
            &party("supplier", "X"),
            None,
        ),
        403,
        "restricted:",
    )
    .await;
    let message = reject(
        &member,
        &entity_event(&member, KIND_CRM_PARTY, channel, other, &payload, None),
        409,
        "conflict:",
    )
    .await;
    assert!(message.contains("500100732259"), "{message}");
    let mut bad = party("customer", "Y");
    bad["inn"] = json!("500100732258");
    reject(
        &member,
        &entity_event(&member, KIND_CRM_PARTY, channel, other, &bad, None),
        400,
        "invalid:",
    )
    .await;
    reject(
        &member,
        &entity_event(
            &member,
            KIND_CRM_PARTY,
            channel,
            maria,
            &payload,
            Some(&"ab".repeat(32)),
        ),
        409,
        "conflict:",
    )
    .await;

    accept(
        &member,
        &entity_event(
            &member,
            KIND_CRM_PARTY,
            channel,
            other,
            &party("supplier", "Молокозавод"),
            None,
        ),
    )
    .await;
    let (_, list) = get(&member, "/crm/parties?q=000-11").await;
    assert_eq!(list["parties"].as_array().unwrap().len(), 2);
    let (_, list) = get(&member, "/crm/parties?role=suppliers").await;
    let names: Vec<&str> = list["parties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Молокозавод"]);

    let mut archived = payload.clone();
    archived["archived"] = json!(true);
    accept(
        &member,
        &entity_event(
            &member,
            KIND_CRM_PARTY,
            channel,
            maria,
            &archived,
            Some(&create.id.to_hex()),
        ),
    )
    .await;
    let (_, list) = get(&member, "/crm/parties?role=customers").await;
    assert!(list["parties"].as_array().unwrap().is_empty());
    let (_, list) = get(&member, "/crm/parties?role=customers&archived=true").await;
    assert_eq!(list["parties"][0]["archived"], json!(true));
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn sales_post_once_validate_references_void_and_feed_analytics() {
    let pool = pool().await;
    let (owner, channel, warehouse) = space(&pool).await;
    let member = Keys::generate();
    add_member(&owner, channel, &member, "member").await;
    let kefir = product(&owner, channel, "Кефир", "piece").await;
    let cheese = product(&owner, channel, "Сыр", "kilogram").await;
    let maria = Uuid::new_v4();
    accept(
        &owner,
        &entity_event(
            &owner,
            KIND_CRM_PARTY,
            channel,
            maria,
            &party("customer", "Мария"),
            None,
        ),
    )
    .await;
    let supplier = Uuid::new_v4();
    accept(
        &owner,
        &entity_event(
            &owner,
            KIND_CRM_PARTY,
            channel,
            supplier,
            &party("supplier", "Завод"),
            None,
        ),
    )
    .await;

    // 2 × 90.00 − 10.00 + 0.75 kg × 90.00 = 237.50.
    let lines = json!([
        {"product_id": kefir, "qty_milli": 2000, "price_minor": 9000, "discount_minor": 1000},
        {"product_id": cheese, "qty_milli": 750, "price_minor": 9000}
    ]);
    let first = Uuid::new_v4();
    let event = entity_event(
        &member,
        KIND_CRM_SALE,
        channel,
        first,
        &sale(warehouse, Some(maria), lines.clone()),
        None,
    );
    accept(&member, &event).await;
    let (status, body) = get(&member, &format!("/crm/sales/{first}")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["number"], json!(1));
    assert_eq!(body["total_minor"], json!(23_750));
    assert_eq!(body["discount_minor"], json!(1_000));
    assert_eq!(body["customer_name"], json!("Мария"));
    assert_eq!(body["lines"][0]["product_name"], json!("Кефир"));
    assert_eq!(body["lines"][1]["amount_minor"], json!(6_750));

    // Re-sending is a duplicate; a new event for the same sale id conflicts.
    let (_, accepted, message) = submit(&member, &event).await;
    assert!(accepted && message.starts_with("duplicate:"), "{message}");
    reject(
        &member,
        &entity_event(
            &member,
            KIND_CRM_SALE,
            channel,
            first,
            &sale(warehouse, None, lines.clone()),
            None,
        ),
        409,
        "conflict:",
    )
    .await;

    // References are checked against the space.
    let half_kefir = json!([{"product_id": kefir, "qty_milli": 500, "price_minor": 9000}]);
    let unknown = json!([{"product_id": Uuid::new_v4(), "qty_milli": 1000, "price_minor": 1}]);
    for payload in [
        sale(warehouse, None, half_kefir),
        sale(warehouse, None, unknown),
        sale(Uuid::new_v4(), None, lines.clone()),
        sale(warehouse, Some(supplier), lines.clone()),
        sale(warehouse, Some(Uuid::new_v4()), lines.clone()),
    ] {
        reject(
            &member,
            &entity_event(
                &member,
                KIND_CRM_SALE,
                channel,
                Uuid::new_v4(),
                &payload,
                None,
            ),
            400,
            "invalid:",
        )
        .await;
    }

    let second = Uuid::new_v4();
    accept(
        &member,
        &entity_event(
            &member,
            KIND_CRM_SALE,
            channel,
            second,
            &sale(
                warehouse,
                None,
                json!([{"product_id": kefir, "qty_milli": 1000, "price_minor": 9000}]),
            ),
            None,
        ),
    )
    .await;

    let (_, party_body) = get(&member, &format!("/crm/parties/{maria}")).await;
    assert_eq!(party_body["purchases"]["revenue_minor"], json!(23_750));

    let (_, analytics) = get(&member, "/crm/analytics").await;
    assert_eq!(
        analytics["totals"]["revenue_minor"],
        json!(32_750),
        "{analytics}"
    );
    assert_eq!(analytics["totals"]["sales_count"], json!(2));
    assert_eq!(analytics["totals"]["avg_check_minor"], json!(16_375));
    assert_eq!(analytics["timezone"], json!("Europe/Moscow"));
    assert_eq!(analytics["top_products"][0]["name"], json!("Кефир"));
    assert_eq!(
        analytics["customers"]["anonymous_revenue_minor"],
        json!(9_000)
    );

    // Members may not void; admins void once; voided sales leave analytics.
    let void = |keys: &Keys, reason: &str| {
        sign(
            keys,
            KIND_CRM_SALE_VOID,
            &json!({"v": 1, "reason": reason}).to_string(),
            vec![
                tag(&["h", &channel.to_string()]),
                tag(&["crm-id", &first.to_string()]),
            ],
        )
    };
    reject(&member, &void(&member, "ошибка"), 403, "restricted:").await;
    accept(&owner, &void(&owner, "ошибка кассира")).await;
    reject(&owner, &void(&owner, "ещё раз"), 409, "conflict:").await;
    let (_, body) = get(&member, &format!("/crm/sales/{first}")).await;
    assert_eq!(body["void"]["reason"], json!("ошибка кассира"));
    let (_, analytics) = get(&member, "/crm/analytics").await;
    assert_eq!(analytics["totals"]["revenue_minor"], json!(9_000));
    let (_, journal) = get(&member, "/crm/sales?voided=false").await;
    assert_eq!(journal["sales"].as_array().unwrap().len(), 1);
    let (_, journal) = get(&member, "/crm/sales").await;
    assert_eq!(journal["sales"][0]["number"], json!(2));
    assert_eq!(journal["sales"][1]["number"], json!(1));

    let (status, _) = get(&member, "/crm/analytics?from=2026-02-01&to=2026-01-01").await;
    assert_eq!(status, 400);
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn reads_require_space_membership() {
    let pool = pool().await;
    let (_, _, _) = space(&pool).await;
    let stranger = Keys::generate();
    for path in ["/crm/parties", "/crm/sales", "/crm/analytics"] {
        let (status, _) = get(&stranger, path).await;
        assert_eq!(status, 403, "{path}");
    }
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn split_payments_receipts_and_shifts_are_recorded() {
    let pool = pool().await;
    let (owner, channel, warehouse) = space(&pool).await;
    let cashier = Keys::generate();
    let bot = Keys::generate();
    add_member(&owner, channel, &cashier, "member").await;
    add_member(&owner, channel, &bot, "bot").await;
    let kefir = product(&owner, channel, "Кефир", "piece").await;

    let mut payload = sale(
        warehouse,
        None,
        json!([{"product_id": kefir, "qty_milli": 3000, "price_minor": 9000}]),
    );
    payload["payment"] = json!("cash");
    payload["payments"] = json!([
        {"method": "cash", "amount_minor": 20_000},
        {"method": "card", "amount_minor": 7_000}
    ]);
    payload["cash_tendered_minor"] = json!(50_000);
    let sale_id = Uuid::new_v4();
    accept(
        &cashier,
        &entity_event(&cashier, KIND_CRM_SALE, channel, sale_id, &payload, None),
    )
    .await;
    let mut wrong = payload.clone();
    wrong["payments"][1]["amount_minor"] = json!(6_999);
    reject(
        &cashier,
        &entity_event(
            &cashier,
            KIND_CRM_SALE,
            channel,
            Uuid::new_v4(),
            &wrong,
            None,
        ),
        400,
        "invalid:",
    )
    .await;

    let shift = |action: &str, document: u64| {
        sign(
            &cashier,
            KIND_CRM_CASH_SHIFT,
            &json!({"v": 1, "action": action, "fn_number": "9999078900004792",
                    "shift_number": 1, "document_number": document,
                    "document_datetime": "2026-10-25T09:00:00+00:00"})
            .to_string(),
            vec![tag(&["h", &channel.to_string()])],
        )
    };
    accept(&cashier, &shift("open", 1)).await;
    let receipt = |keys: &Keys, total: i64, document: u64| {
        sign(
            keys,
            KIND_CRM_FISCAL_RECEIPT,
            &json!({"v": 1, "operation": "sell", "total_minor": total,
                    "task_uuid": Uuid::new_v4(),
                    "fiscal": {"document_number": document, "document_sign": "3826380862",
                               "receipt_number": 1, "shift_number": 1,
                               "fn_number": "9999078900004792",
                               "document_datetime": "2026-10-25T10:00:00+00:00"}})
            .to_string(),
            vec![
                tag(&["h", &channel.to_string()]),
                tag(&["crm-id", &sale_id.to_string()]),
            ],
        )
    };
    // Bots may not record fiscal documents; totals must match the sale.
    reject(&bot, &receipt(&bot, 27_000, 2), 403, "restricted:").await;
    reject(&cashier, &receipt(&cashier, 26_999, 2), 400, "invalid:").await;
    accept(&cashier, &receipt(&cashier, 27_000, 2)).await;
    reject(&cashier, &receipt(&cashier, 27_000, 3), 409, "conflict:").await;

    let (_, body) = get(&cashier, &format!("/crm/sales/{sale_id}")).await;
    assert_eq!(body["cash_tendered_minor"], json!(50_000), "{body}");
    assert_eq!(
        body["payments"],
        json!([{"method": "cash", "amount_minor": 20_000}, {"method": "card", "amount_minor": 7_000}])
    );
    assert_eq!(body["fiscal"]["sell"]["document_number"], json!(2));
    let (_, analytics) = get(&cashier, "/crm/analytics").await;
    let payments: Vec<(String, i64)> = analytics["by_payment"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            (
                b["key"].as_str().unwrap().to_string(),
                b["revenue_minor"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        payments,
        vec![("cash".into(), 20_000), ("card".into(), 7_000)]
    );

    accept(&cashier, &shift("close", 3)).await;
    let (status, shifts) = get(&cashier, "/crm/shifts").await;
    assert_eq!(status, 200);
    assert_eq!(shifts["shifts"][0]["closed"], json!(true));
    assert_eq!(shifts["shifts"][0]["sell_total_minor"], json!(27_000));
    assert_eq!(shifts["shifts"][0]["cash_minor"], json!(20_000));
}
