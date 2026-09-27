//! End-to-end tests for the CRM catalog (kind 42102, `GET /crm/products`).
//!
//! Covers `.specs/features/crm-catalog/spec.md`: product create/update CAS,
//! SKU and barcode uniqueness, role checks, search, barcode lookup, archive,
//! pagination and read access.
//!
//! # Running
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 \
//! DATABASE_URL=postgres://buzz:buzz_dev@localhost:5432/buzz \
//!   cargo test -p buzz-test-client --test e2e_crm_catalog -- --ignored --test-threads=1
//! ```

mod crm_support;

use crm_support::*;
use nostr::Keys;
use serde_json::{json, Value};
use uuid::Uuid;

fn product(name: &str, sku: Option<&str>, barcodes: &[&str]) -> Value {
    json!({
        "v": 1,
        "name": name,
        "sku": sku,
        "barcodes": barcodes,
        "unit": "piece",
        "price_minor": 123_450,
        "vat": "vat22",
        "payment_object": "commodity"
    })
}

fn product_event(
    keys: &Keys,
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
    sign(keys, KIND_CRM_PRODUCT, &payload.to_string(), tags)
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

async fn names(keys: &Keys, query: &str) -> Vec<String> {
    let (status, body) = get(Some(keys), &format!("/crm/products{query}")).await;
    assert_eq!(status, 200, "GET /crm/products{query}: {body}");
    body["products"]
        .as_array()
        .expect("products array")
        .iter()
        .map(|p| p["name"].as_str().unwrap_or_default().to_string())
        .collect()
}

async fn accept(keys: &Keys, event: &nostr::Event) {
    let (status, accepted, message) = submit(keys, event).await;
    assert!(
        status == 200 && accepted && message.is_empty(),
        "{status} {message}"
    );
}

/// Owner (community admin) with a fresh CRM space channel.
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
#[ignore = "requires a running relay and its Postgres"]
async fn products_are_created_updated_and_kept_unique() {
    let pool = pool().await;
    let (owner, channel, community) = space(&pool).await;
    let member = Keys::generate();
    add_member(&owner, channel, &member, "member").await;

    // A member creates a product; the read model mirrors the event.
    let tea = Uuid::new_v4();
    let create = product_event(
        &member,
        channel,
        tea,
        &product(" Чай зелёный ", Some("TEA-1"), &["4006381333931"]),
        None,
    );
    accept(&member, &create).await;
    let (status, body) = get(Some(&member), &format!("/crm/products/{tea}")).await;
    assert_eq!(status, 200);
    assert_eq!(body["name"], json!("Чай зелёный"));
    assert_eq!(body["sku"], json!("TEA-1"));
    assert_eq!(body["barcodes"], json!(["4006381333931"]));
    assert_eq!(body["price_minor"], json!(123_450));
    assert_eq!(body["revision"], json!(create.id.to_hex()));
    assert_eq!(body["archived"], json!(false));

    // The identical event again is a duplicate.
    let (_, accepted, message) = submit(&member, &create).await;
    assert!(accepted && message == "duplicate:", "{message}");

    // A re-signed create for the same id is a conflict, not an update.
    let recreate = product_event(&member, channel, tea, &product("Чай", None, &[]), None);
    let (status, accepted, message) = submit(&member, &recreate).await;
    assert!(
        status == 409 && !accepted && message.starts_with("conflict:"),
        "{status} {message}"
    );

    // SKU (case-insensitive) and barcodes are unique per space.
    let coffee = Uuid::new_v4();
    let (status, _, message) = submit(
        &member,
        &product_event(
            &member,
            channel,
            coffee,
            &product("Кофе", Some("tea-1"), &[]),
            None,
        ),
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(
        message,
        "conflict: SKU \"tea-1\" is already used by another product"
    );
    let (status, _, message) = submit(
        &member,
        &product_event(
            &member,
            channel,
            coffee,
            &product("Кофе", Some("CF-1"), &["INT-1", "4006381333931"]),
            None,
        ),
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(
        message,
        "conflict: barcode 4006381333931 is already used by another product"
    );

    // Validation failures are invalid.
    let (_, accepted, message) = submit(
        &member,
        &product_event(
            &member,
            channel,
            coffee,
            &product("Кофе", None, &["4006381333932"]),
            None,
        ),
    )
    .await;
    assert!(!accepted && message.starts_with("invalid: "), "{message}");

    // Stale revision → conflict, row unchanged; current revision → updated.
    let stale = product_event(
        &member,
        channel,
        tea,
        &product("Чай чёрный", Some("TEA-1"), &[]),
        Some(&"00".repeat(32)),
    );
    let (status, _, message) = submit(&member, &stale).await;
    assert!(
        status == 409 && message == "conflict: product changed since it was loaded",
        "{status} {message}"
    );
    let update = product_event(
        &owner,
        channel,
        tea,
        &product("Чай чёрный", Some("TEA-1"), &[]),
        Some(&create.id.to_hex()),
    );
    accept(&owner, &update).await;
    let (_, body) = get(Some(&member), &format!("/crm/products/{tea}")).await;
    assert_eq!(body["name"], json!("Чай чёрный"));
    assert_eq!(body["barcodes"], json!([]));
    assert_eq!(body["revision"], json!(update.id.to_hex()));

    // The released barcode can now be used by another product.
    accept(
        &member,
        &product_event(
            &member,
            channel,
            coffee,
            &product("Кофе", Some("CF-1"), &["4006381333931"]),
            None,
        ),
    )
    .await;

    // Guests cannot edit the catalog; bots can.
    let guest = Keys::generate();
    let bot = Keys::generate();
    add_member(&owner, channel, &guest, "guest").await;
    add_member(&owner, channel, &bot, "bot").await;
    let (_, accepted, message) = submit(
        &guest,
        &product_event(
            &guest,
            channel,
            Uuid::new_v4(),
            &product("Гость", None, &[]),
            None,
        ),
    )
    .await;
    assert!(!accepted && message.starts_with("restricted:"), "{message}");
    accept(
        &bot,
        &product_event(
            &bot,
            channel,
            Uuid::new_v4(),
            &product("Бот", None, &[]),
            None,
        ),
    )
    .await;

    // A product event outside the CRM space is invalid.
    let other = create_channel(&owner, "private").await;
    let (_, accepted, message) = submit(
        &owner,
        &product_event(
            &owner,
            other,
            Uuid::new_v4(),
            &product("X", None, &[]),
            None,
        ),
    )
    .await;
    assert!(
        !accepted && message == "invalid: this channel is not the CRM space",
        "{message}"
    );

    // A removed member's next write is rejected (role read in the write tx).
    let removal = sign(
        &owner,
        9001,
        "",
        vec![
            tag(&["h", &channel.to_string()]),
            tag(&["p", &member.public_key().to_hex()]),
        ],
    );
    accept(&owner, &removal).await;
    let (_, accepted, _) = submit(
        &member,
        &product_event(
            &member,
            channel,
            Uuid::new_v4(),
            &product("После удаления", None, &[]),
            None,
        ),
    )
    .await;
    assert!(!accepted, "removed member must not write");

    // Product history cannot be deleted, not even by its author.
    let delete = sign(
        &owner,
        5,
        "",
        vec![
            tag(&["h", &channel.to_string()]),
            tag(&["e", &update.id.to_hex()]),
        ],
    );
    let (_, accepted, message) = submit(&owner, &delete).await;
    assert!(
        !accepted && message.starts_with("invalid: CRM events cannot be deleted"),
        "{message}"
    );

    cleanup_space(&pool, community).await;
}

#[tokio::test]
#[ignore = "requires a running relay and its Postgres"]
async fn products_are_searched_paginated_archived_and_member_only() {
    let pool = pool().await;
    let (owner, channel, community) = space(&pool).await;

    let mut ids = Vec::new();
    for (name, sku, barcodes) in [
        ("Яблоко", Some("APL-1"), vec!["96385074"]),
        ("банан", None, vec![]),
        ("Вишня 50%", Some("CHR-7"), vec![]),
        ("груша", None, vec!["INT-42"]),
    ] {
        let id = Uuid::new_v4();
        accept(
            &owner,
            &product_event(&owner, channel, id, &product(name, sku, &barcodes), None),
        )
        .await;
        ids.push(id);
    }

    assert_eq!(
        names(&owner, "").await,
        vec!["банан", "Вишня 50%", "груша", "Яблоко"]
    );
    assert_eq!(names(&owner, "?q=%D0%93%D0%A0%D0%A3").await, vec!["груша"]);
    assert_eq!(names(&owner, "?q=apl").await, vec!["Яблоко"]);
    assert_eq!(names(&owner, "?q=50%25").await, vec!["Вишня 50%"]);
    assert_eq!(names(&owner, "?barcode=96385074").await, vec!["Яблоко"]);
    assert!(names(&owner, "?barcode=00000000").await.is_empty());

    // Keyset pages.
    let (_, first) = get(Some(&owner), "/crm/products?limit=3").await;
    assert_eq!(first["products"].as_array().unwrap().len(), 3);
    let next = first["next"].as_str().expect("next cursor").to_string();
    let (_, second) = get(Some(&owner), &format!("/crm/products?limit=3&after={next}")).await;
    assert_eq!(second["products"][0]["name"], json!("Яблоко"));
    assert_eq!(second["next"], Value::Null);

    // Archive hides from default lists; archived=true shows it.
    let (_, pear) = get(Some(&owner), &format!("/crm/products/{}", ids[3])).await;
    let mut archived = product("груша", None, &["INT-42"]);
    archived["archived"] = json!(true);
    accept(
        &owner,
        &product_event(
            &owner,
            channel,
            ids[3],
            &archived,
            Some(pear["revision"].as_str().unwrap()),
        ),
    )
    .await;
    assert!(!names(&owner, "").await.contains(&"груша".to_string()));
    assert!(names(&owner, "?archived=true")
        .await
        .contains(&"груша".to_string()));
    // Archived products keep their barcodes reserved.
    let (status, _, _) = submit(
        &owner,
        &product_event(
            &owner,
            channel,
            Uuid::new_v4(),
            &product("Новая груша", None, &["INT-42"]),
            None,
        ),
    )
    .await;
    assert_eq!(status, 409);

    // Bad parameters are rejected; unknown ids are 404.
    let (status, _) = get(Some(&owner), "/crm/products?limit=500").await;
    assert_eq!(status, 400);
    let (status, _) = get(Some(&owner), &format!("/crm/products/{}", Uuid::new_v4())).await;
    assert_eq!(status, 404);

    // Only space members read the catalog.
    let outsider = Keys::generate();
    let (status, _) = get(Some(&outsider), "/crm/products").await;
    assert_eq!(status, 403);
    let (status, _) = get(Some(&outsider), &format!("/crm/products/{}", ids[0])).await;
    assert_eq!(status, 403);
    let (status, _) = get(None, "/crm/products").await;
    assert_eq!(status, 401);

    cleanup_space(&pool, community).await;
}
