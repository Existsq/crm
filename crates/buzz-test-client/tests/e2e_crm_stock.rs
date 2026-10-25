//! End-to-end tests for the stock ledger: stock documents (kind 42106) and
//! their voids (42107), service tech cards (42108), the stock movements of
//! sales (42104, 42105) and the `GET /crm/stock*`, `/crm/tech-cards` reads.
//!
//! # Running
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 \
//! DATABASE_URL=postgres://buzz:buzz_dev@localhost:5432/buzz \
//!   cargo test -p buzz-test-client --test e2e_crm_stock -- --ignored --test-threads=1
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

async fn ok(keys: &Keys, path_and_query: &str) -> Value {
    let (status, body) = get(keys, path_and_query).await;
    assert_eq!(status, 200, "GET {path_and_query}: {body}");
    body
}

async fn accept(keys: &Keys, event: &nostr::Event) {
    let (status, accepted, message) = submit(keys, event).await;
    assert!(
        status == 200 && accepted && message.is_empty(),
        "{status} {message}"
    );
}

async fn reject(keys: &Keys, event: &nostr::Event, needle: &str) -> String {
    let (status, accepted, message) = submit(keys, event).await;
    assert!(
        !accepted && status >= 400 && message.contains(needle),
        "expected a rejection containing {needle:?}, got {status} {message}"
    );
    message
}

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

async fn product(keys: &Keys, channel: Uuid, name: &str, unit: &str, object: &str) -> Uuid {
    let id = Uuid::new_v4();
    let payload = json!({
        "v": 1, "name": name, "unit": unit, "price_minor": 50_000,
        "vat": "none", "payment_object": object, "min_stock_milli": 2_000
    });
    accept(
        keys,
        &entity_event(keys, KIND_CRM_PRODUCT, channel, id, &payload, None),
    )
    .await;
    id
}

fn doc(keys: &Keys, channel: Uuid, payload: Value) -> (Uuid, nostr::Event) {
    let id = Uuid::new_v4();
    (
        id,
        entity_event(keys, KIND_CRM_STOCK_DOC, channel, id, &payload, None),
    )
}

fn void(keys: &Keys, kind: u16, channel: Uuid, id: Uuid) -> nostr::Event {
    entity_event(
        keys,
        kind,
        channel,
        id,
        &json!({"v": 1, "reason": "ошибка"}),
        None,
    )
}

async fn level(keys: &Keys, warehouse: Uuid, product: Uuid) -> (i64, i64) {
    let body = ok(keys, &format!("/crm/stock?warehouse={warehouse}")).await;
    let item = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["product_id"] == json!(product))
        .cloned()
        .unwrap_or_else(|| panic!("{product} not in {body}"));
    (
        item["qty_milli"].as_i64().unwrap(),
        item["value_minor"].as_i64().unwrap(),
    )
}

fn sale(warehouse: Uuid, lines: Value) -> Value {
    json!({"v": 1, "warehouse_id": warehouse, "payment": "cash", "lines": lines})
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn documents_move_stock_at_average_cost_and_voids_revert_them() {
    let pool = pool().await;
    let (owner, channel, main) = space(&pool).await;
    let member = Keys::generate();
    add_member(&owner, channel, &member, "member").await;
    let shampoo = product(&owner, channel, "Шампунь", "piece", "commodity").await;
    let haircut = product(&owner, channel, "Стрижка", "service_unit", "service").await;
    let supplier = Uuid::new_v4();
    accept(
        &owner,
        &entity_event(
            &owner,
            KIND_CRM_PARTY,
            channel,
            supplier,
            &json!({"v": 1, "type": "supplier", "name": "ООО Косметика"}),
            None,
        ),
    )
    .await;
    let annex = Uuid::new_v4();
    accept(
        &owner,
        &entity_event(
            &owner,
            KIND_CRM_WAREHOUSE,
            channel,
            annex,
            &json!({"v": 1, "name": "Кабинет 2"}),
            None,
        ),
    )
    .await;

    // Two receipts at different costs average out: 10 × 200 + 10 × 300.
    for cost in [20_000, 30_000] {
        let (_, receipt) = doc(
            &member,
            channel,
            json!({"v": 1, "type": "receipt", "warehouse_id": main, "supplier_id": supplier,
                   "reference": "ТН-7",
                   "lines": [{"product_id": shampoo, "qty_milli": 10_000, "cost_minor": cost}]}),
        );
        accept(&member, &receipt).await;
    }
    assert_eq!(level(&owner, main, shampoo).await, (20_000, 500_000));

    // Services are not stocked; a customer cannot supply; ids post once.
    let (_, service_line) = doc(
        &member,
        channel,
        json!({"v": 1, "type": "receipt", "warehouse_id": main,
               "lines": [{"product_id": haircut, "qty_milli": 1_000, "cost_minor": 1}]}),
    );
    reject(&member, &service_line, "not stocked").await;

    // Selling more than is on hand is refused; selling 4 takes 4 × 250.
    let over = entity_event(
        &member,
        KIND_CRM_SALE,
        channel,
        Uuid::new_v4(),
        &sale(
            main,
            json!([{"product_id": shampoo, "qty_milli": 21_000, "price_minor": 50_000}]),
        ),
        None,
    );
    reject(&member, &over, "not enough stock of \"Шампунь\"").await;
    let sale_id = Uuid::new_v4();
    accept(
        &member,
        &entity_event(
            &member,
            KIND_CRM_SALE,
            channel,
            sale_id,
            &sale(
                main,
                json!([{"product_id": shampoo, "qty_milli": 4_000, "price_minor": 50_000}]),
            ),
            None,
        ),
    )
    .await;
    assert_eq!(level(&owner, main, shampoo).await, (16_000, 400_000));

    // Write-off, transfer and inventory.
    let (write_off, event) = doc(
        &member,
        channel,
        json!({"v": 1, "type": "write_off", "warehouse_id": main, "note": "протёк",
               "lines": [{"product_id": shampoo, "qty_milli": 1_000}]}),
    );
    accept(&member, &event).await;
    let (_, event) = doc(
        &member,
        channel,
        json!({"v": 1, "type": "transfer", "warehouse_id": main, "to_warehouse_id": annex,
               "lines": [{"product_id": shampoo, "qty_milli": 5_000}]}),
    );
    accept(&member, &event).await;
    assert_eq!(level(&owner, main, shampoo).await, (10_000, 250_000));
    assert_eq!(level(&owner, annex, shampoo).await, (5_000, 125_000));
    let (_, event) = doc(
        &member,
        channel,
        json!({"v": 1, "type": "inventory", "warehouse_id": annex,
               "lines": [{"product_id": shampoo, "qty_milli": 3_000}]}),
    );
    accept(&member, &event).await;
    assert_eq!(level(&owner, annex, shampoo).await, (3_000, 75_000));
    let all = ok(&owner, "/crm/stock").await;
    assert_eq!(all["totals"]["value_minor"], json!(325_000));

    // Voids: members may not, admins may, once; the write-off comes back.
    let undo = void(&member, KIND_CRM_STOCK_DOC_VOID, channel, write_off);
    reject(&member, &undo, "").await;
    accept(
        &owner,
        &void(&owner, KIND_CRM_STOCK_DOC_VOID, channel, write_off),
    )
    .await;
    assert_eq!(level(&owner, main, shampoo).await, (11_000, 275_000));
    let again = entity_event(
        &owner,
        KIND_CRM_STOCK_DOC_VOID,
        channel,
        write_off,
        &json!({"v": 1, "reason": "повтор"}),
        None,
    );
    reject(&owner, &again, "already voided").await;

    // Voiding the sale returns its goods.
    accept(&owner, &void(&owner, KIND_CRM_SALE_VOID, channel, sale_id)).await;
    assert_eq!(level(&owner, main, shampoo).await, (15_000, 375_000));

    // Journal and documents read back with numbers and lines.
    let docs = ok(&owner, "/crm/stock/docs").await;
    let docs = docs["docs"].as_array().unwrap().clone();
    assert_eq!(docs.len(), 5);
    assert_eq!(docs[0]["number"], json!(5));
    assert_eq!(docs[4]["supplier_name"], json!("ООО Косметика"));
    let first = ok(
        &owner,
        &format!("/crm/stock/docs/{}", docs[4]["id"].as_str().unwrap()),
    )
    .await;
    assert_eq!(first["lines"][0]["cost_minor"], json!(20_000));
    let receipts = ok(&owner, "/crm/stock/docs?type=receipt").await;
    assert_eq!(receipts["docs"].as_array().unwrap().len(), 2);
    let moves = ok(
        &owner,
        &format!("/crm/stock/moves?product={shampoo}&limit=3"),
    )
    .await;
    let moves_list = moves["moves"].as_array().unwrap();
    assert_eq!(moves_list[0]["source"], json!("sale"));
    assert_eq!(moves_list[0]["reversal"], json!(true));
    assert!(moves["next"].is_number());
    let (status, _) = get(&owner, "/crm/stock/docs?type=gift").await;
    assert_eq!(status, 400);
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn tech_cards_write_off_consumables_when_services_are_sold() {
    let pool = pool().await;
    let (owner, channel, main) = space(&pool).await;
    let dye = product(&owner, channel, "Краска", "gram", "commodity").await;
    let gloves = product(&owner, channel, "Перчатки", "piece", "commodity").await;
    let coloring = product(&owner, channel, "Окрашивание", "service_unit", "service").await;
    stock_up(&owner, channel, main, &[dye]).await;

    let card = |items: Value| json!({"v": 1, "items": items});
    reject(
        &owner,
        &entity_event(
            &owner,
            KIND_CRM_TECH_CARD,
            channel,
            dye,
            &card(json!([])),
            None,
        ),
        "not a service",
    )
    .await;
    reject(
        &owner,
        &entity_event(
            &owner,
            KIND_CRM_TECH_CARD,
            channel,
            coloring,
            &card(json!([{"product_id": gloves, "qty_milli": 500}])),
            None,
        ),
        "whole units",
    )
    .await;
    let create = entity_event(
        &owner,
        KIND_CRM_TECH_CARD,
        channel,
        coloring,
        &card(json!([{"product_id": dye, "qty_milli": 60_000}])),
        None,
    );
    accept(&owner, &create).await;
    reject(
        &owner,
        &entity_event(
            &owner,
            KIND_CRM_TECH_CARD,
            channel,
            coloring,
            &card(json!([])),
            None,
        ),
        "already has a tech card",
    )
    .await;
    accept(
        &owner,
        &entity_event(
            &owner,
            KIND_CRM_TECH_CARD,
            channel,
            coloring,
            &card(json!([
                {"product_id": dye, "qty_milli": 60_000},
                {"product_id": gloves, "qty_milli": 1_000}
            ])),
            Some(&create.id.to_hex()),
        ),
    )
    .await;
    let cards = ok(&owner, "/crm/tech-cards").await;
    assert_eq!(cards["cards"][0]["service_name"], json!("Окрашивание"));
    assert_eq!(cards["cards"][0]["items"].as_array().unwrap().len(), 2);

    // Two colorings use 120 g of dye and 2 pairs of gloves; gloves were never
    // received, but consumables never block a paid visit.
    let visit = Uuid::new_v4();
    accept(
        &owner,
        &entity_event(
            &owner,
            KIND_CRM_SALE,
            channel,
            visit,
            &sale(
                main,
                json!([{"product_id": coloring, "qty_milli": 2_000, "price_minor": 300_000}]),
            ),
            None,
        ),
    )
    .await;
    assert_eq!(level(&owner, main, dye).await, (-20_000, 0));
    assert_eq!(level(&owner, main, gloves).await, (-2_000, 0));
    let moves = ok(&owner, &format!("/crm/stock/moves?product={dye}")).await;
    assert_eq!(moves["moves"][0]["source"], json!("consumable"));
    assert_eq!(moves["moves"][0]["qty_delta_milli"], json!(-120_000));

    // Voiding the visit puts the consumables back.
    accept(&owner, &void(&owner, KIND_CRM_SALE_VOID, channel, visit)).await;
    assert_eq!(level(&owner, main, dye).await.0, 100_000);
    assert_eq!(level(&owner, main, gloves).await.0, 0);
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn a_receipt_whose_goods_were_sold_cannot_be_voided() {
    let pool = pool().await;
    let (owner, channel, main) = space(&pool).await;
    let wax = product(&owner, channel, "Воск", "piece", "commodity").await;
    let (receipt, event) = doc(
        &owner,
        channel,
        json!({"v": 1, "type": "receipt", "warehouse_id": main,
               "lines": [{"product_id": wax, "qty_milli": 2_000, "cost_minor": 10_000}]}),
    );
    accept(&owner, &event).await;
    accept(
        &owner,
        &entity_event(
            &owner,
            KIND_CRM_SALE,
            channel,
            Uuid::new_v4(),
            &sale(
                main,
                json!([{"product_id": wax, "qty_milli": 1_000, "price_minor": 50_000}]),
            ),
            None,
        ),
    )
    .await;
    reject(
        &owner,
        &void(&owner, KIND_CRM_STOCK_DOC_VOID, channel, receipt),
        "already used",
    )
    .await;
    assert_eq!(level(&owner, main, wax).await, (1_000, 10_000));

    let guest = Keys::generate();
    add_member(&owner, channel, &guest, "guest").await;
    let (_, event) = doc(
        &guest,
        channel,
        json!({"v": 1, "type": "write_off", "warehouse_id": main,
               "lines": [{"product_id": wax, "qty_milli": 1_000}]}),
    );
    reject(&guest, &event, "").await;
    let (status, _) = get(&Keys::generate(), "/crm/stock").await;
    assert_eq!(status, 403);
}
