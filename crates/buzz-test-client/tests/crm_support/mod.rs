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
