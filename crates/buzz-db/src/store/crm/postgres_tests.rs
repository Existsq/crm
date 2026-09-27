use super::*;
use buzz_core::channel::{ChannelType, ChannelVisibility};
use serde_json::json;

struct Fixture {
    db: Db,
    community: CommunityId,
    channel: Uuid,
    owner: Vec<u8>,
}

async fn fixture(visibility: ChannelVisibility) -> Fixture {
    let pool = PgPool::connect(&crate::test_support::database_url())
        .await
        .expect("connect test database");
    if std::env::var("BUZZ_TEST_SCHEMA_MODE").as_deref() == Ok("migration") {
        crate::migration::run_migrations(&pool)
            .await
            .expect("migrate");
    }
    let db = Db::from_pool(pool);
    let community = db
        .ensure_configured_community(&format!("crm-{}.example", Uuid::new_v4()))
        .await
        .expect("community")
        .id;
    let owner = vec![7_u8; 32];
    let channel = Uuid::new_v4();
    db.create_channel_with_id(
        community,
        channel,
        "crm",
        ChannelType::Stream,
        visibility,
        None,
        &owner,
        None,
    )
    .await
    .expect("channel");
    Fixture {
        db,
        community,
        channel,
        owner,
    }
}

fn settings() -> serde_json::Value {
    json!({
        "currency": "RUB",
        "timezone": "Europe/Moscow",
        "allow_negative_stock": false,
        "fiscalization": "required",
        "default_vat": "vat22"
    })
}

fn rev(byte: u8) -> Vec<u8> {
    vec![byte; 32]
}

async fn create(fx: &Fixture, warehouse: Uuid) -> SpaceCreateOutcome {
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = create_space(
        &mut tx,
        fx.community,
        fx.channel,
        &settings(),
        &rev(1),
        &fx.owner,
        warehouse,
        "Основной склад",
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    outcome
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn create_space_inserts_space_and_default_warehouse() {
    let fx = fixture(ChannelVisibility::Private).await;
    let warehouse = Uuid::new_v4();
    assert_eq!(create(&fx, warehouse).await, SpaceCreateOutcome::Created);

    let space = fx.db.crm_get_space(fx.community).await.unwrap().unwrap();
    assert_eq!(space.channel_id, fx.channel);
    assert_eq!(space.settings, settings());
    assert_eq!(space.revision_event_id, rev(1));

    let warehouses = fx
        .db
        .crm_list_warehouses(fx.community, fx.channel, false)
        .await
        .unwrap();
    assert_eq!(warehouses.len(), 1);
    assert_eq!(warehouses[0].warehouse_id, warehouse);
    assert!(warehouses[0].is_default);
    assert!(fx
        .db
        .crm_is_space_channel(fx.community, fx.channel)
        .await
        .unwrap());
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn second_space_in_community_is_refused() {
    let fx = fixture(ChannelVisibility::Private).await;
    assert_eq!(
        create(&fx, Uuid::new_v4()).await,
        SpaceCreateOutcome::Created
    );

    let other_channel = Uuid::new_v4();
    fx.db
        .create_channel_with_id(
            fx.community,
            other_channel,
            "crm-2",
            ChannelType::Stream,
            ChannelVisibility::Private,
            None,
            &fx.owner,
            None,
        )
        .await
        .unwrap();
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = create_space(
        &mut tx,
        fx.community,
        other_channel,
        &settings(),
        &rev(2),
        &fx.owner,
        Uuid::new_v4(),
        "Второй",
    )
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(outcome, SpaceCreateOutcome::CommunityHasSpace);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn settings_update_is_compare_and_swap() {
    let fx = fixture(ChannelVisibility::Private).await;
    create(&fx, Uuid::new_v4()).await;
    let mut changed = settings();
    changed["allow_negative_stock"] = json!(true);

    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let stale = update_space_settings(
        &mut tx,
        fx.community,
        fx.channel,
        &changed,
        &rev(9),
        &rev(2),
    )
    .await
    .unwrap();
    assert_eq!(stale, CasOutcome::RevisionMismatch);
    let fresh = update_space_settings(
        &mut tx,
        fx.community,
        fx.channel,
        &changed,
        &rev(1),
        &rev(2),
    )
    .await
    .unwrap();
    assert_eq!(fresh, CasOutcome::Updated);
    let missing = update_space_settings(
        &mut tx,
        fx.community,
        Uuid::new_v4(),
        &changed,
        &rev(2),
        &rev(3),
    )
    .await
    .unwrap();
    assert_eq!(missing, CasOutcome::Missing);
    tx.commit().await.unwrap();

    let space = fx.db.crm_get_space(fx.community).await.unwrap().unwrap();
    assert_eq!(space.settings["allow_negative_stock"], json!(true));
    assert_eq!(space.revision_event_id, rev(2));
}

fn write<'a>(
    fx: &Fixture,
    id: Uuid,
    name: &'a str,
    default: bool,
    archived: bool,
    revision: &'a [u8],
) -> WarehouseWrite<'a> {
    WarehouseWrite {
        warehouse_id: id,
        space_channel_id: fx.channel,
        name,
        is_default: default,
        archived,
        revision_event_id: revision,
    }
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn warehouses_keep_exactly_one_default() {
    let fx = fixture(ChannelVisibility::Private).await;
    let main = Uuid::new_v4();
    create(&fx, main).await;
    let shop = Uuid::new_v4();
    let r2 = rev(2);
    let r3 = rev(3);
    let r4 = rev(4);

    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    // New non-default warehouse.
    assert_eq!(
        write_warehouse(
            &mut tx,
            fx.community,
            write(&fx, shop, "Магазин", false, false, &r2),
            None
        )
        .await
        .unwrap(),
        WarehouseWriteOutcome::Written
    );
    // Creating it again is refused.
    assert_eq!(
        write_warehouse(
            &mut tx,
            fx.community,
            write(&fx, shop, "Магазин", false, false, &r3),
            None
        )
        .await
        .unwrap(),
        WarehouseWriteOutcome::AlreadyExists
    );
    // Clearing the default on the current default is refused.
    assert_eq!(
        write_warehouse(
            &mut tx,
            fx.community,
            write(&fx, main, "Склад", false, false, &r3),
            Some(&rev(1))
        )
        .await
        .unwrap(),
        WarehouseWriteOutcome::WouldRemoveDefault
    );
    // Archiving the current default is refused.
    assert_eq!(
        write_warehouse(
            &mut tx,
            fx.community,
            write(&fx, main, "Склад", false, true, &r3),
            Some(&rev(1))
        )
        .await
        .unwrap(),
        WarehouseWriteOutcome::WouldRemoveDefault
    );
    // Stale revision.
    assert_eq!(
        write_warehouse(
            &mut tx,
            fx.community,
            write(&fx, shop, "Магазин", true, false, &r3),
            Some(&rev(9))
        )
        .await
        .unwrap(),
        WarehouseWriteOutcome::RevisionMismatch
    );
    // Moving the default to the shop clears it on the main warehouse.
    assert_eq!(
        write_warehouse(
            &mut tx,
            fx.community,
            write(&fx, shop, "Магазин", true, false, &r3),
            Some(&r2)
        )
        .await
        .unwrap(),
        WarehouseWriteOutcome::Written
    );
    // Update of an unknown warehouse.
    assert_eq!(
        write_warehouse(
            &mut tx,
            fx.community,
            write(&fx, Uuid::new_v4(), "X", false, false, &r4),
            Some(&r2)
        )
        .await
        .unwrap(),
        WarehouseWriteOutcome::Missing
    );
    // Demoting the old default gave it the demoting event's revision, so a
    // client still holding its pre-move revision cannot re-promote it.
    assert_eq!(
        write_warehouse(
            &mut tx,
            fx.community,
            write(&fx, main, "Склад", true, false, &r4),
            Some(&rev(1))
        )
        .await
        .unwrap(),
        WarehouseWriteOutcome::RevisionMismatch
    );
    // With the current revision the former default can now be archived.
    assert_eq!(
        write_warehouse(
            &mut tx,
            fx.community,
            write(&fx, main, "Склад", false, true, &r4),
            Some(&r3)
        )
        .await
        .unwrap(),
        WarehouseWriteOutcome::Written
    );
    tx.commit().await.unwrap();

    let visible = fx
        .db
        .crm_list_warehouses(fx.community, fx.channel, false)
        .await
        .unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].warehouse_id, shop);
    assert!(visible[0].is_default);
    let all = fx
        .db
        .crm_list_warehouses(fx.community, fx.channel, true)
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
    assert!(all
        .iter()
        .any(|w| w.warehouse_id == main && w.archived && !w.is_default));
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn authorization_reads_see_removals_and_channel_facts() {
    let fx = fixture(ChannelVisibility::Private).await;
    let member = vec![9_u8; 32];
    sqlx::query(
        "INSERT INTO channel_members (community_id, channel_id, pubkey, role) \
         VALUES ($1, $2, $3, 'member')",
    )
    .bind(fx.community.as_uuid())
    .bind(fx.channel)
    .bind(&member)
    .execute(&fx.db.pool)
    .await
    .unwrap();

    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    assert_eq!(
        channel_role_for_write(&mut tx, fx.community, fx.channel, &fx.owner)
            .await
            .unwrap()
            .as_deref(),
        Some("owner")
    );
    assert_eq!(
        channel_role_for_write(&mut tx, fx.community, fx.channel, &member)
            .await
            .unwrap()
            .as_deref(),
        Some("member")
    );
    let facts = channel_facts_for_write(&mut tx, fx.community, fx.channel)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(facts.visibility, "private");
    assert_eq!(facts.channel_type, "stream");
    assert!(!facts.archived && !facts.has_ttl);
    assert!(
        channel_facts_for_write(&mut tx, fx.community, Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
    assert!(timezone_exists(&mut tx, "Europe/Moscow").await.unwrap());
    assert!(!timezone_exists(&mut tx, "Mars/Olympus").await.unwrap());
    tx.rollback().await.unwrap();

    sqlx::query(
        "UPDATE channel_members SET removed_at = now() \
         WHERE community_id = $1 AND channel_id = $2 AND pubkey = $3",
    )
    .bind(fx.community.as_uuid())
    .bind(fx.channel)
    .bind(&member)
    .execute(&fx.db.pool)
    .await
    .unwrap();
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    assert_eq!(
        channel_role_for_write(&mut tx, fx.community, fx.channel, &member)
            .await
            .unwrap(),
        None,
        "a removed member has no write role"
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn spaces_are_community_scoped() {
    let a = fixture(ChannelVisibility::Private).await;
    let b = fixture(ChannelVisibility::Private).await;
    create(&a, Uuid::new_v4()).await;
    assert!(b.db.crm_get_space(b.community).await.unwrap().is_none());
    assert!(!b
        .db
        .crm_is_space_channel(b.community, a.channel)
        .await
        .unwrap());
    assert!(b
        .db
        .crm_list_warehouses(b.community, a.channel, true)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn concurrent_creates_of_one_warehouse_id_admit_exactly_one() {
    let fx = fixture(ChannelVisibility::Private).await;
    create(&fx, Uuid::new_v4()).await;
    let id = Uuid::new_v4();
    let (first, second) = (rev(20), rev(21));

    let mut tx1 = fx.db.begin_event_write_transaction().await.unwrap();
    assert_eq!(
        write_warehouse(
            &mut tx1,
            fx.community,
            write(&fx, id, "A", false, false, &first),
            None
        )
        .await
        .unwrap(),
        WarehouseWriteOutcome::Written
    );

    // The second create cannot see the uncommitted row and blocks on the key.
    let db = fx.db.clone();
    let community = fx.community;
    let channel = fx.channel;
    let racer = tokio::spawn(async move {
        let mut tx2 = db.begin_event_write_transaction().await.unwrap();
        let outcome = write_warehouse(
            &mut tx2,
            community,
            WarehouseWrite {
                warehouse_id: id,
                space_channel_id: channel,
                name: "B",
                is_default: false,
                archived: false,
                revision_event_id: &second,
            },
            None,
        )
        .await
        .unwrap();
        tx2.commit().await.unwrap();
        outcome
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    tx1.commit().await.unwrap();

    assert_eq!(racer.await.unwrap(), WarehouseWriteOutcome::AlreadyExists);
    let rows = fx
        .db
        .crm_list_warehouses(fx.community, fx.channel, true)
        .await
        .unwrap();
    let row = rows.iter().find(|w| w.warehouse_id == id).unwrap();
    assert_eq!(
        row.name, "A",
        "the losing create must not overwrite the winner"
    );
    assert_eq!(row.revision_event_id, first);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn space_channel_cannot_be_opened_expired_archived_or_deleted() {
    let fx = fixture(ChannelVisibility::Private).await;
    create(&fx, Uuid::new_v4()).await;
    let update = |sql: &'static str| {
        let pool = fx.db.pool.clone();
        let community = fx.community;
        let channel = fx.channel;
        async move {
            sqlx::query(sql)
                .bind(community.as_uuid())
                .bind(channel)
                .execute(&pool)
                .await
        }
    };
    for sql in [
        "UPDATE channels SET visibility = 'open' WHERE community_id = $1 AND id = $2",
        "UPDATE channels SET ttl_seconds = 3600 WHERE community_id = $1 AND id = $2",
        "UPDATE channels SET archived_at = now() WHERE community_id = $1 AND id = $2",
        "UPDATE channels SET deleted_at = now() WHERE community_id = $1 AND id = $2",
    ] {
        let error = update(sql).await.expect_err(sql);
        assert!(
            error.to_string().contains("hosts the CRM space"),
            "{sql}: {error}"
        );
    }
    // Ordinary metadata edits of the space channel still work.
    update("UPDATE channels SET name = 'crm-renamed' WHERE community_id = $1 AND id = $2")
        .await
        .expect("rename");

    // Channels that are not the CRM space are unaffected.
    let other = Uuid::new_v4();
    fx.db
        .create_channel_with_id(
            fx.community,
            other,
            "other",
            ChannelType::Stream,
            ChannelVisibility::Private,
            None,
            &fx.owner,
            None,
        )
        .await
        .unwrap();
    sqlx::query("UPDATE channels SET visibility = 'open' WHERE community_id = $1 AND id = $2")
        .bind(fx.community.as_uuid())
        .bind(other)
        .execute(&fx.db.pool)
        .await
        .expect("non-CRM channel may open");
}
