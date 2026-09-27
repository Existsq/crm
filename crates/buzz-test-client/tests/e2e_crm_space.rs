//! End-to-end tests for the CRM space (kinds 42100/42101, `GET /crm/space`).
//!
//! Covers `.specs/features/crm-foundation/spec.md`: who may bind a private
//! channel as the CRM space, duplicate handling, member vs. non-member reads,
//! warehouse CAS and role checks, and the guards that keep CRM history and the
//! space channel intact.
//!
//! The community-admin role lives in `relay_members`, which the tests seed
//! through Postgres (the same database the relay under test uses).
//!
//! # Running
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 \
//! DATABASE_URL=postgres://buzz:buzz_dev@localhost:5432/buzz \
//!   cargo test -p buzz-test-client --test e2e_crm_space -- --ignored --test-threads=1
//! ```
//!
//! Each test needs a community without a CRM space, so the tests archive any
//! space they create before finishing (`cleanup_space`).

mod crm_support;

use crm_support::*;
use nostr::Keys;
use serde_json::{json, Value};
use uuid::Uuid;

fn warehouse_event(
    keys: &Keys,
    channel: Uuid,
    id: Uuid,
    name: &str,
    expected_revision: Option<&str>,
) -> nostr::Event {
    let mut tags = vec![
        tag(&["h", &channel.to_string()]),
        tag(&["crm-id", &id.to_string()]),
    ];
    if let Some(revision) = expected_revision {
        tags.push(tag(&["expected-revision", revision]));
    }
    sign(
        keys,
        KIND_CRM_WAREHOUSE,
        &json!({"v": 1, "name": name}).to_string(),
        tags,
    )
}

fn warehouse_event_with(
    keys: &Keys,
    channel: Uuid,
    id: Uuid,
    payload: Value,
    expected_revision: Option<&str>,
) -> nostr::Event {
    let mut tags = vec![
        tag(&["h", &channel.to_string()]),
        tag(&["crm-id", &id.to_string()]),
    ];
    if let Some(revision) = expected_revision {
        tags.push(tag(&["expected-revision", revision]));
    }
    sign(keys, KIND_CRM_WAREHOUSE, &payload.to_string(), tags)
}

fn warehouse_revision(space: &Value, id: Uuid) -> Option<String> {
    space["warehouses"]
        .as_array()?
        .iter()
        .find(|w| w["id"] == json!(id))
        .and_then(|w| w["revision"].as_str().map(str::to_string))
}

async fn get_space_status(keys: Option<&Keys>) -> u16 {
    let mut request = http().get(format!("{}/crm/space", relay_http_url()));
    if let Some(keys) = keys {
        request = request.header("X-Pubkey", keys.public_key().to_hex());
    }
    request
        .send()
        .await
        .expect("GET /crm/space")
        .status()
        .as_u16()
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn crm_space_lifecycle_and_access() {
    let pool = pool().await;
    let owner = Keys::generate();
    let member = Keys::generate();
    let outsider = Keys::generate();

    let channel = create_channel(&owner, "private").await;
    let community = community_of(&pool, channel).await;
    cleanup_space(&pool, community).await;

    // A channel owner who is not a community admin cannot bind the space.
    let warehouse = Uuid::new_v4();
    let (_, accepted, message) = submit(&owner, &space_create(&owner, channel, warehouse)).await;
    assert!(!accepted && message.starts_with("restricted:"), "{message}");

    // Without a space, a community admin learns they may create one.
    grant_community_role(&pool, community, &owner, "admin").await;
    let before = get_space(&owner).await;
    assert_eq!(before, json!({"exists": false, "can_create": true}));
    let outsider_before = get_space(&outsider).await;
    assert_eq!(outsider_before["can_create"], json!(false));

    // Community admin + channel owner binds it.
    let create = space_create(&owner, channel, warehouse);
    let (status, accepted, message) = submit(&owner, &create).await;
    assert!(
        status == 200 && accepted && message.is_empty(),
        "{status} {message}"
    );

    // Re-sending the identical event is a duplicate, not a second space.
    let (_, accepted, message) = submit(&owner, &create).await;
    assert!(accepted && message == "duplicate:", "{message}");

    // Member view.
    let space = get_space(&owner).await;
    assert_eq!(space["member"], json!(true));
    assert_eq!(space["channel_id"], json!(channel));
    assert_eq!(space["my_role"], json!("owner"));
    assert_eq!(space["settings"], settings());
    assert_eq!(space["revision"], json!(create.id.to_hex()));
    assert_eq!(space["warehouses"][0]["id"], json!(warehouse));
    assert_eq!(space["warehouses"][0]["is_default"], json!(true));

    // Non-member view reveals only existence.
    assert_eq!(
        get_space(&outsider).await,
        json!({"exists": true, "member": false})
    );

    // A second space in the same community is a conflict.
    let other = create_channel(&owner, "private").await;
    let (status, accepted, message) =
        submit(&owner, &space_create(&owner, other, Uuid::new_v4())).await;
    assert!(
        !accepted && message.starts_with("conflict:"),
        "{status} {message}"
    );
    assert_eq!(status, 409);

    // Members read but cannot manage warehouses.
    add_member(&owner, channel, &member, "member").await;
    let member_view = get_space(&member).await;
    assert_eq!(member_view["my_role"], json!("member"));
    assert!(member_view["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("post_documents")));
    let shop = Uuid::new_v4();
    let (_, accepted, message) = submit(
        &member,
        &warehouse_event(&member, channel, shop, "Магазин", None),
    )
    .await;
    assert!(!accepted && message.starts_with("restricted:"), "{message}");

    // Outsiders cannot write into the space at all.
    let (_, accepted, _) = submit(
        &outsider,
        &warehouse_event(&outsider, channel, shop, "Чужой", None),
    )
    .await;
    assert!(!accepted);

    // Owner creates a warehouse; a stale update conflicts; a fresh one lands.
    let created = warehouse_event(&owner, channel, shop, "Магазин", None);
    let (_, accepted, message) = submit(&owner, &created).await;
    assert!(accepted, "{message}");
    let stale = warehouse_event(&owner, channel, shop, "Магазин 2", Some(&"00".repeat(32)));
    let (status, accepted, message) = submit(&owner, &stale).await;
    assert!(
        !accepted && message.starts_with("conflict:") && status == 409,
        "{message}"
    );
    let fresh = warehouse_event(
        &owner,
        channel,
        shop,
        "Магазин на Ленина",
        Some(&created.id.to_hex()),
    );
    let (_, accepted, message) = submit(&owner, &fresh).await;
    assert!(accepted, "{message}");
    let names: Vec<String> = get_space(&owner).await["warehouses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        names.contains(&"Магазин на Ленина".to_string()),
        "{names:?}"
    );

    // A settings update with the current revision replaces the settings.
    let mut changed = settings();
    changed["allow_negative_stock"] = json!(true);
    let update = sign(
        &owner,
        KIND_CRM_SPACE,
        &json!({"v": 1, "settings": changed}).to_string(),
        vec![
            tag(&["h", &channel.to_string()]),
            tag(&["expected-revision", &create.id.to_hex()]),
        ],
    );
    let (_, accepted, message) = submit(&owner, &update).await;
    assert!(accepted, "{message}");
    assert_eq!(
        get_space(&owner).await["settings"]["allow_negative_stock"],
        json!(true)
    );

    // Guards: the space channel stays private and CRM history is immutable.
    let open_it = sign(
        &owner,
        9002,
        "",
        vec![
            tag(&["h", &channel.to_string()]),
            tag(&["visibility", "open"]),
        ],
    );
    let (_, accepted, message) = submit(&owner, &open_it).await;
    assert!(
        !accepted && message == "invalid: the CRM space channel must stay private",
        "{message}"
    );
    let delete_history = sign(&owner, 5, "", vec![tag(&["e", &create.id.to_hex()])]);
    let (_, accepted, message) = submit(&owner, &delete_history).await;
    assert!(
        !accepted && message.starts_with("invalid: CRM events cannot be deleted"),
        "{message}"
    );

    cleanup_space(&pool, community).await;
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn crm_space_rejects_open_channels_and_bad_payloads() {
    let pool = pool().await;
    let owner = Keys::generate();
    let open_channel = create_channel(&owner, "open").await;
    let community = community_of(&pool, open_channel).await;
    cleanup_space(&pool, community).await;
    grant_community_role(&pool, community, &owner, "owner").await;

    let (_, accepted, message) =
        submit(&owner, &space_create(&owner, open_channel, Uuid::new_v4())).await;
    assert!(
        !accepted && message == "invalid: a CRM space channel must be private",
        "{message}"
    );

    let private_channel = create_channel(&owner, "private").await;
    let bad_tz = sign(
        &owner,
        KIND_CRM_SPACE,
        &json!({
            "v": 1,
            "settings": {
                "currency": "RUB", "timezone": "Mars/Olympus", "allow_negative_stock": false,
                "fiscalization": "off", "default_vat": "vat22"
            },
            "default_warehouse": {"id": Uuid::new_v4(), "name": "Склад"}
        })
        .to_string(),
        vec![tag(&["h", &private_channel.to_string()])],
    );
    let (_, accepted, message) = submit(&owner, &bad_tz).await;
    assert!(
        !accepted && message.starts_with("invalid:") && message.contains("unknown timezone"),
        "{message}"
    );

    let future_version = sign(
        &owner,
        KIND_CRM_SPACE,
        &json!({"v": 2}).to_string(),
        vec![tag(&["h", &private_channel.to_string()])],
    );
    let (_, accepted, message) = submit(&owner, &future_version).await;
    assert!(
        !accepted
            && message.starts_with("invalid:")
            && message.contains("unsupported CRM payload version 2"),
        "{message}"
    );

    let no_h = sign(&owner, KIND_CRM_SPACE, &json!({"v": 1}).to_string(), vec![]);
    let (_, accepted, message) = submit(&owner, &no_h).await;
    assert!(
        !accepted && message.starts_with("invalid:") && message.contains("h tag"),
        "{message}"
    );

    assert!(get_space(&owner).await["exists"] == json!(false));
    cleanup_space(&pool, community).await;
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn crm_space_cas_roles_and_guards() {
    let pool = pool().await;
    let owner = Keys::generate();
    let guest = Keys::generate();
    let bot = Keys::generate();
    let member = Keys::generate();

    let channel = create_channel(&owner, "private").await;
    let community = community_of(&pool, channel).await;
    cleanup_space(&pool, community).await;
    grant_community_role(&pool, community, &owner, "owner").await;

    // Reads need authentication, like the workflow read API.
    assert_eq!(get_space_status(None).await, 401);

    let main = Uuid::new_v4();
    let create = space_create(&owner, channel, main);
    let (_, accepted, message) = submit(&owner, &create).await;
    assert!(accepted, "{message}");

    // A settings update advances the revision to its own event id; a second
    // update still holding the old revision is a 409 conflict.
    let mut changed = settings();
    changed["fiscalization"] = json!("off");
    let update = |revision: &str, value: &Value| {
        sign(
            &owner,
            KIND_CRM_SPACE,
            &json!({"v": 1, "settings": value}).to_string(),
            vec![
                tag(&["h", &channel.to_string()]),
                tag(&["expected-revision", revision]),
            ],
        )
    };
    let first = update(&create.id.to_hex(), &changed);
    let (_, accepted, message) = submit(&owner, &first).await;
    assert!(accepted, "{message}");
    assert_eq!(
        get_space(&owner).await["revision"],
        json!(first.id.to_hex())
    );
    let (status, accepted, message) =
        submit(&owner, &update(&create.id.to_hex(), &settings())).await;
    assert!(
        !accepted && status == 409 && message.starts_with("conflict:"),
        "{status} {message}"
    );

    // The default warehouse cannot be cleared or archived directly.
    let clear_default = warehouse_event_with(
        &owner,
        channel,
        main,
        json!({"v": 1, "name": "Основной склад", "is_default": false}),
        Some(&create.id.to_hex()),
    );
    let (_, accepted, message) = submit(&owner, &clear_default).await;
    assert!(
        !accepted && message == "invalid: make another warehouse the default first",
        "{message}"
    );

    // Moving the default gives the demoted warehouse a new revision, so the
    // pre-move revision can no longer re-promote it (lost update).
    let shop = Uuid::new_v4();
    let shop_create = warehouse_event_with(
        &owner,
        channel,
        shop,
        json!({"v": 1, "name": "Магазин"}),
        None,
    );
    assert!(submit(&owner, &shop_create).await.1);
    let promote = warehouse_event_with(
        &owner,
        channel,
        shop,
        json!({"v": 1, "name": "Магазин", "is_default": true}),
        Some(&shop_create.id.to_hex()),
    );
    let (_, accepted, message) = submit(&owner, &promote).await;
    assert!(accepted, "{message}");
    let space = get_space(&owner).await;
    assert_eq!(warehouse_revision(&space, main), Some(promote.id.to_hex()));
    let stale_repromote = warehouse_event_with(
        &owner,
        channel,
        main,
        json!({"v": 1, "name": "Основной склад", "is_default": true}),
        Some(&create.id.to_hex()),
    );
    let (status, accepted, message) = submit(&owner, &stale_repromote).await;
    assert!(
        !accepted && status == 409 && message.starts_with("conflict:"),
        "{message}"
    );

    // An archived warehouse disappears from the read model.
    let archive_main = warehouse_event_with(
        &owner,
        channel,
        main,
        json!({"v": 1, "name": "Основной склад", "archived": true}),
        Some(&promote.id.to_hex()),
    );
    let (_, accepted, message) = submit(&owner, &archive_main).await;
    assert!(accepted, "{message}");
    assert_eq!(warehouse_revision(&get_space(&owner).await, main), None);
    // ...but `?archived=true` still lists it so it can be restored.
    let with_archived = get_json(&owner, "/crm/space?archived=true").await;
    let archived_main = with_archived["warehouses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == json!(main))
        .expect("archived warehouse listed");
    assert_eq!(archived_main["archived"], json!(true));
    assert_eq!(archived_main["revision"], json!(archive_main.id.to_hex()));

    // Guests and bots cannot manage warehouses; members cannot change settings.
    add_member(&owner, channel, &guest, "guest").await;
    add_member(&owner, channel, &bot, "bot").await;
    add_member(&owner, channel, &member, "member").await;
    for keys in [&guest, &bot] {
        let event = warehouse_event_with(
            keys,
            channel,
            Uuid::new_v4(),
            json!({"v": 1, "name": "X"}),
            None,
        );
        let (_, accepted, message) = submit(keys, &event).await;
        assert!(!accepted && message.starts_with("restricted:"), "{message}");
    }
    let member_update = sign(
        &member,
        KIND_CRM_SPACE,
        &json!({"v": 1, "settings": settings()}).to_string(),
        vec![
            tag(&["h", &channel.to_string()]),
            tag(&["expected-revision", &first.id.to_hex()]),
        ],
    );
    let (_, accepted, message) = submit(&member, &member_update).await;
    assert!(!accepted && message.starts_with("restricted:"), "{message}");

    // A CRM write into a channel that is not the CRM space is invalid.
    let other = create_channel(&owner, "private").await;
    let stray = warehouse_event_with(
        &owner,
        other,
        Uuid::new_v4(),
        json!({"v": 1, "name": "X"}),
        None,
    );
    let (_, accepted, message) = submit(&owner, &stray).await;
    assert!(
        !accepted && message == "invalid: this channel is not the CRM space",
        "{message}"
    );

    // Channel admins cannot delete CRM history through kind 9005 either.
    let admin_delete = sign(
        &owner,
        9005,
        "",
        vec![
            tag(&["h", &channel.to_string()]),
            tag(&["e", &shop_create.id.to_hex()]),
        ],
    );
    let (_, accepted, message) = submit(&owner, &admin_delete).await;
    assert!(
        !accepted && message.starts_with("invalid: CRM events cannot be deleted"),
        "{message}"
    );

    cleanup_space(&pool, community).await;
}

async fn get_json(keys: &Keys, path_and_query: &str) -> Value {
    let resp = http()
        .get(format!("{}{path_and_query}", relay_http_url()))
        .header("X-Pubkey", keys.public_key().to_hex())
        .send()
        .await
        .expect("GET");
    assert_eq!(resp.status().as_u16(), 200, "GET {path_and_query}");
    resp.json().await.expect("json")
}

/// Concurrent moves of the default flag to different warehouses must all
/// succeed and leave exactly one default: warehouse writes serialize on the
/// space row. Without that lock the demote-then-set steps interleave and the
/// one-default unique index turns a valid move into an internal error.
#[tokio::test]
#[ignore = "requires a running relay"]
async fn crm_space_concurrent_default_moves_serialize() {
    let pool = pool().await;
    let owner = Keys::generate();
    let channel = create_channel(&owner, "private").await;
    let community = community_of(&pool, channel).await;
    cleanup_space(&pool, community).await;
    grant_community_role(&pool, community, &owner, "admin").await;
    let (_, accepted, message) =
        submit(&owner, &space_create(&owner, channel, Uuid::new_v4())).await;
    assert!(accepted, "{message}");

    let mut warehouses = Vec::new();
    for index in 0..8 {
        let id = Uuid::new_v4();
        let event = warehouse_event(&owner, channel, id, &format!("Склад {index}"), None);
        let (_, accepted, message) = submit(&owner, &event).await;
        assert!(accepted, "{message}");
        warehouses.push((id, event.id.to_hex()));
    }

    for round in 0..3 {
        let space = get_space(&owner).await;
        let moves: Vec<nostr::Event> = warehouses
            .iter()
            .map(|(id, _)| {
                warehouse_event_with(
                    &owner,
                    channel,
                    *id,
                    json!({"v": 1, "name": format!("Склад {id}"), "is_default": true}),
                    Some(&warehouse_revision(&space, *id).expect("live warehouse")),
                )
            })
            .collect();
        let results =
            futures_util::future::join_all(moves.iter().map(|event| submit(&owner, event))).await;
        for (status, accepted, message) in &results {
            // A move may lose CAS to a concurrent demotion of the same
            // warehouse (409); it must never fail inside the relay.
            assert!(
                *accepted || (*status == 409 && message.starts_with("conflict:")),
                "round {round}: {status} {message}"
            );
        }
        assert!(results.iter().any(|(_, accepted, _)| *accepted));
        let defaults = get_space(&owner).await["warehouses"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|w| w["is_default"] == json!(true))
            .count();
        assert_eq!(defaults, 1, "round {round}");
    }

    cleanup_space(&pool, community).await;
}
