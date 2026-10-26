//! Shared helpers for the CRM relay e2e tests (`e2e_crm_*.rs`).
//!
//! Requests use the relay's dev `X-Pubkey` auth; community roles are seeded in
//! the relay's Postgres (`DATABASE_URL`).
#![allow(dead_code)] // Each e2e binary uses a different subset.

use std::time::Duration;

use nostr::{EventBuilder, Keys, Kind, Tag};
use reqwest::Client;
use serde_json::{json, Value};
use uuid::Uuid;

pub const KIND_CRM_SPACE: u16 = 42100;
pub const KIND_CRM_WAREHOUSE: u16 = 42101;
pub const KIND_CRM_PRODUCT: u16 = 42102;
pub const KIND_CRM_PARTY: u16 = 42103;
pub const KIND_CRM_SALE: u16 = 42104;
pub const KIND_CRM_SALE_VOID: u16 = 42105;
pub const KIND_CRM_STOCK_DOC: u16 = 42106;
pub const KIND_CRM_STOCK_DOC_VOID: u16 = 42107;
pub const KIND_CRM_TECH_CARD: u16 = 42108;
pub const KIND_CRM_FISCAL_RECEIPT: u16 = 42120;
pub const KIND_CRM_CASH_SHIFT: u16 = 42121;
pub const KIND_CRM_STAFF: u16 = 42131;
pub const KIND_CRM_STAFF_SCHEDULE: u16 = 42132;
pub const KIND_CRM_APPOINTMENT: u16 = 42133;
pub const KIND_CRM_REMINDER_SETTINGS: u16 = 42150;
pub const KIND_CRM_CLIENT_TELEGRAM: u16 = 42151;

pub fn relay_url() -> String {
    std::env::var("RELAY_URL").unwrap_or_else(|_| "ws://localhost:3000".to_string())
}

pub fn relay_http_url() -> String {
    relay_url()
        .replace("wss://", "https://")
        .replace("ws://", "http://")
        .trim_end_matches('/')
        .to_string()
}

pub fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://buzz:buzz_dev@localhost:5432/buzz".to_string())
}

pub fn http() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("http client")
}

pub async fn pool() -> sqlx::PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url())
        .await
        .expect("connect to the relay's Postgres")
}

pub fn tag(parts: &[&str]) -> Tag {
    Tag::parse(parts.iter().copied()).expect("tag")
}

pub fn sign(keys: &Keys, kind: u16, content: &str, tags: Vec<Tag>) -> nostr::Event {
    EventBuilder::new(Kind::Custom(kind), content)
        .tags(tags)
        .sign_with_keys(keys)
        .expect("sign")
}

/// Submit via the HTTP bridge (dev `X-Pubkey` auth). Returns `(status, accepted, message)`.
pub async fn submit(keys: &Keys, event: &nostr::Event) -> (u16, bool, String) {
    let resp = http()
        .post(format!("{}/events", relay_http_url()))
        .header("X-Pubkey", keys.public_key().to_hex())
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(event).unwrap())
        .send()
        .await
        .expect("submit event");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    let accepted = body["accepted"].as_bool().unwrap_or(false);
    let message = body["message"]
        .as_str()
        .or_else(|| body["error"].as_str())
        .unwrap_or("")
        .to_string();
    (status, accepted, message)
}

pub async fn get_space(keys: &Keys) -> Value {
    let resp = http()
        .get(format!("{}/crm/space", relay_http_url()))
        .header("X-Pubkey", keys.public_key().to_hex())
        .send()
        .await
        .expect("GET /crm/space");
    assert_eq!(resp.status().as_u16(), 200, "GET /crm/space status");
    resp.json().await.expect("space json")
}

pub async fn create_channel(keys: &Keys, visibility: &str) -> Uuid {
    let channel = Uuid::new_v4();
    let event = sign(
        keys,
        9007,
        "",
        vec![
            tag(&["h", &channel.to_string()]),
            tag(&["name", &format!("crm-e2e-{}", channel.simple())]),
            tag(&["channel_type", "stream"]),
            tag(&["visibility", visibility]),
        ],
    );
    let (status, accepted, message) = submit(keys, &event).await;
    assert!(
        status == 200 && accepted,
        "create channel: {status} {message}"
    );
    channel
}

pub async fn add_member(owner: &Keys, channel: Uuid, member: &Keys, role: &str) {
    let event = sign(
        owner,
        9000,
        "",
        vec![
            tag(&["h", &channel.to_string()]),
            tag(&["p", &member.public_key().to_hex()]),
            tag(&["role", role]),
        ],
    );
    let (status, accepted, message) = submit(owner, &event).await;
    assert!(status == 200 && accepted, "add member: {status} {message}");
}

pub async fn community_of(pool: &sqlx::PgPool, channel: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT community_id FROM channels WHERE id = $1")
        .bind(channel)
        .fetch_one(pool)
        .await
        .expect("channel community")
}

pub async fn grant_community_role(pool: &sqlx::PgPool, community: Uuid, keys: &Keys, role: &str) {
    sqlx::query(
        "INSERT INTO relay_members (community_id, pubkey, role) VALUES ($1, $2, $3) \
         ON CONFLICT (community_id, pubkey) DO UPDATE SET role = EXCLUDED.role",
    )
    .bind(community)
    .bind(keys.public_key().to_hex())
    .bind(role)
    .execute(pool)
    .await
    .expect("seed relay member");
}

/// Remove the space so later tests start from "no space". Deleting projection
/// rows directly is test-only; the relay never deletes CRM rows.
pub async fn cleanup_space(pool: &sqlx::PgPool, community: Uuid) {
    for sql in [
        "DELETE FROM crm_tech_card_items WHERE community_id = $1",
        "DELETE FROM crm_tech_cards WHERE community_id = $1",
        "DELETE FROM crm_stock_moves WHERE community_id = $1",
        "DELETE FROM crm_stock WHERE community_id = $1",
        "DELETE FROM crm_stock_doc_lines WHERE community_id = $1",
        "DELETE FROM crm_stock_docs WHERE community_id = $1",
        "DELETE FROM crm_fiscal_receipts WHERE community_id = $1",
        "DELETE FROM crm_cash_shifts WHERE community_id = $1",
        "DELETE FROM crm_sale_payments WHERE community_id = $1",
        "DELETE FROM crm_sale_lines WHERE community_id = $1",
        "DELETE FROM crm_sales WHERE community_id = $1",
        "DELETE FROM crm_reminder_deliveries WHERE community_id = $1",
        "DELETE FROM crm_reminder_settings WHERE community_id = $1",
        "DELETE FROM crm_appointments WHERE community_id = $1",
        "DELETE FROM crm_client_telegram WHERE community_id = $1",
        "DELETE FROM crm_parties WHERE community_id = $1",
        "DELETE FROM crm_staff_days WHERE community_id = $1",
        "DELETE FROM crm_staff WHERE community_id = $1",
    ] {
        sqlx::query(sql)
            .bind(community)
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
    sqlx::query("DELETE FROM crm_product_barcodes WHERE community_id = $1")
        .bind(community)
        .execute(pool)
        .await
        .expect("cleanup barcodes");
    sqlx::query("DELETE FROM crm_products WHERE community_id = $1")
        .bind(community)
        .execute(pool)
        .await
        .expect("cleanup products");
    sqlx::query("DELETE FROM crm_warehouses WHERE community_id = $1")
        .bind(community)
        .execute(pool)
        .await
        .expect("cleanup warehouses");
    sqlx::query("DELETE FROM crm_spaces WHERE community_id = $1")
        .bind(community)
        .execute(pool)
        .await
        .expect("cleanup spaces");
}

pub fn settings() -> Value {
    json!({
        "currency": "RUB",
        "timezone": "Europe/Moscow",
        "allow_negative_stock": false,
        "fiscalization": "optional",
        "default_vat": "vat22"
    })
}

pub fn space_create(keys: &Keys, channel: Uuid, warehouse: Uuid) -> nostr::Event {
    let content = json!({
        "v": 1,
        "settings": settings(),
        "default_warehouse": {"id": warehouse, "name": "Основной склад"}
    });
    sign(
        keys,
        KIND_CRM_SPACE,
        &content.to_string(),
        vec![tag(&["h", &channel.to_string()])],
    )
}

/// A CRM event on `channel`, with the optional `crm-id` and
/// `expected-revision` tags.
pub fn crm_event(
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

/// `GET` a relay path, optionally as `keys`; returns status and JSON body.
pub async fn get(keys: Option<&Keys>, path_and_query: &str) -> (u16, Value) {
    let mut request = http().get(format!("{}{path_and_query}", relay_http_url()));
    if let Some(keys) = keys {
        request = request.header("X-Pubkey", keys.public_key().to_hex());
    }
    let resp = request.send().await.expect("GET");
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// Submit and assert the relay accepted the event.
pub async fn accept(keys: &Keys, event: &nostr::Event) {
    let (status, accepted, message) = submit(keys, event).await;
    assert!(
        status == 200 && accepted && message.is_empty(),
        "{status} {message}"
    );
}

/// Submit and assert the relay rejected the event with `prefix`.
pub async fn reject(keys: &Keys, event: &nostr::Event, prefix: &str) -> String {
    let (status, accepted, message) = submit(keys, event).await;
    assert!(
        !accepted && message.starts_with(prefix),
        "expected {prefix}, got {status} {message}"
    );
    message
}

/// A fresh space owned by a new community admin: `(owner, channel, community)`.
pub async fn fresh_space(pool: &sqlx::PgPool) -> (Keys, Uuid, Uuid) {
    let owner = Keys::generate();
    let channel = create_channel(&owner, "private").await;
    let community = community_of(pool, channel).await;
    cleanup_space(pool, community).await;
    grant_community_role(pool, community, &owner, "admin").await;
    accept(&owner, &space_create(&owner, channel, Uuid::new_v4())).await;
    (owner, channel, community)
}

/// Receive 100 units of each product on `warehouse` (kind 42106), so sales
/// pass the space's no-negative-stock rule.
pub async fn stock_up(keys: &Keys, channel: Uuid, warehouse: Uuid, products: &[Uuid]) {
    let lines: Vec<Value> = products
        .iter()
        .map(|id| json!({"product_id": id, "qty_milli": 100_000, "cost_minor": 1_000}))
        .collect();
    let payload = json!({"v": 1, "type": "receipt", "warehouse_id": warehouse, "lines": lines});
    let event = sign(
        keys,
        KIND_CRM_STOCK_DOC,
        &payload.to_string(),
        vec![
            tag(&["h", &channel.to_string()]),
            tag(&["crm-id", &Uuid::new_v4().to_string()]),
        ],
    );
    let (status, accepted, message) = submit(keys, &event).await;
    assert!(status == 200 && accepted, "stock up: {status} {message}");
}
