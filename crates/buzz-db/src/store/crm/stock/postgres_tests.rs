use super::*;
use crate::crm::catalog::{write_product, ProductWrite, ProductWriteOutcome};
use crate::crm::{create_space, SpaceCreateOutcome};
use buzz_core::channel::{ChannelType, ChannelVisibility};
use serde_json::json;

struct Fixture {
    db: Db,
    community: CommunityId,
    space: Uuid,
    warehouse: Uuid,
    actor: Vec<u8>,
}

async fn fixture() -> Fixture {
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
        .ensure_configured_community(&format!("crm-stock-{}.example", Uuid::new_v4()))
        .await
        .expect("community")
        .id;
    let actor = vec![7_u8; 32];
    let space = Uuid::new_v4();
    let warehouse = Uuid::new_v4();
    db.create_channel_with_id(
        community,
        space,
        "crm",
        ChannelType::Stream,
        ChannelVisibility::Private,
        None,
        &actor,
        None,
    )
    .await
    .expect("channel");
    let mut tx = db.begin_event_write_transaction().await.expect("begin");
    let created = create_space(
        &mut tx,
        community,
        space,
        &json!({"currency": "RUB", "timezone": "Europe/Moscow", "allow_negative_stock": true}),
        &[1; 32],
        &actor,
        warehouse,
        "main",
    )
    .await
    .expect("space");
    assert_eq!(created, SpaceCreateOutcome::Created);
    tx.commit().await.expect("commit");
    Fixture {
        db,
        community,
        space,
        warehouse,
        actor,
    }
}

async fn product(fx: &Fixture, name: &str, payment_object: &str) -> Uuid {
    let id = Uuid::new_v4();
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_product(
        &mut tx,
        fx.community,
        ProductWrite {
            product_id: id,
            space_channel_id: fx.space,
            name,
            sku: None,
            barcodes: &[],
            unit: "piece",
            price_minor: 50_000,
            vat: "none",
            payment_object,
            category: None,
            min_stock_milli: 2_000,
            requires_marking: false,
            duration_min: None,
            archived: false,
            revision_event_id: &[id.as_bytes()[0]; 32],
            actor: &fx.actor,
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(outcome, ProductWriteOutcome::Written);
    tx.commit().await.unwrap();
    id
}

fn mv(
    fx: &Fixture,
    product: Uuid,
    source: &'static str,
    dq: i64,
    after: (i64, i64),
) -> StockMoveWrite {
    StockMoveWrite {
        warehouse_id: fx.warehouse,
        product_id: product,
        source,
        reversal: false,
        qty_delta_milli: dq,
        value_delta_minor: 0,
        qty_after_milli: after.0,
        value_after_minor: after.1,
    }
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn levels_journal_and_documents_round_trip() {
    let fx = fixture().await;
    let shampoo = product(&fx, "Шампунь", "commodity").await;
    let haircut = product(&fx, "Стрижка", "service").await;
    let doc = Uuid::new_v4();

    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    assert!(allows_negative_stock(&mut tx, fx.community, fx.space)
        .await
        .unwrap());
    let levels = lock_levels(&mut tx, fx.community, fx.space, &[(fx.warehouse, shampoo)])
        .await
        .unwrap();
    assert_eq!(levels[&(fx.warehouse, shampoo)], (0, 0));
    let lines = [StockDocLineInsert {
        product_id: shampoo,
        product_name: "Шампунь",
        unit: "piece",
        qty_milli: 5_000,
        book_qty_milli: None,
        cost_minor: Some(20_000),
        amount_minor: 100_000,
    }];
    let number = insert_stock_doc(
        &mut tx,
        fx.community,
        &StockDocInsert {
            doc_id: doc,
            space_channel_id: fx.space,
            doc_type: "receipt",
            warehouse_id: fx.warehouse,
            to_warehouse_id: None,
            supplier_id: None,
            reference: Some("ТН-1"),
            note: None,
            total_minor: 100_000,
            posted_at: Utc::now(),
            event_id: &[2; 32],
            actor: &fx.actor,
            lines: &lines,
        },
    )
    .await
    .unwrap();
    assert_eq!(number, 1);
    let mut receipt = mv(&fx, shampoo, "receipt", 5_000, (5_000, 100_000));
    receipt.value_delta_minor = 100_000;
    write_moves(
        &mut tx,
        fx.community,
        fx.space,
        doc,
        Utc::now(),
        &[2; 32],
        &[receipt],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let stock = fx
        .db
        .crm_list_stock(fx.community, fx.space, &StockQuery::default())
        .await
        .unwrap();
    assert_eq!(stock.items.len(), 1, "services are not listed: {stock:?}");
    assert_eq!(
        (stock.matched, stock.value_minor, stock.low_count),
        (1, 100_000, 0)
    );
    assert_eq!(
        (stock.items[0].qty_milli, stock.items[0].value_minor),
        (5_000, 100_000)
    );
    let low = fx
        .db
        .crm_list_stock(
            fx.community,
            fx.space,
            &StockQuery {
                low_only: true,
                ..StockQuery::default()
            },
        )
        .await
        .unwrap();
    assert!(low.items.is_empty());

    // A second document reads the level the first left and voids it.
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let levels = lock_levels(&mut tx, fx.community, fx.space, &[(fx.warehouse, shampoo)])
        .await
        .unwrap();
    assert_eq!(levels[&(fx.warehouse, shampoo)], (5_000, 100_000));
    assert_eq!(
        void_stock_doc(
            &mut tx,
            fx.community,
            fx.space,
            doc,
            Utc::now(),
            &[3; 32],
            "ошибка",
            &fx.actor
        )
        .await
        .unwrap(),
        StockDocVoidOutcome::Voided
    );
    let forward = moves_of_doc(&mut tx, fx.community, fx.space, doc, &["receipt"])
        .await
        .unwrap();
    assert_eq!(forward.len(), 1);
    let mut back = mv(&fx, shampoo, "receipt", -5_000, (0, 0));
    back.reversal = true;
    back.value_delta_minor = -100_000;
    write_moves(
        &mut tx,
        fx.community,
        fx.space,
        doc,
        Utc::now(),
        &[3; 32],
        &[back],
    )
    .await
    .unwrap();
    assert_eq!(
        void_stock_doc(
            &mut tx,
            fx.community,
            fx.space,
            doc,
            Utc::now(),
            &[4; 32],
            "ещё",
            &fx.actor
        )
        .await
        .unwrap(),
        StockDocVoidOutcome::AlreadyVoided
    );
    assert_eq!(
        moves_of_doc(&mut tx, fx.community, fx.space, doc, &["receipt"])
            .await
            .unwrap()
            .len(),
        1,
        "reversals are not forward moves"
    );
    tx.commit().await.unwrap();

    let page = fx
        .db
        .crm_list_stock_moves(
            fx.community,
            fx.space,
            &StockMoveQuery {
                product_id: Some(shampoo),
                limit: 1,
                ..StockMoveQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.moves.len(), 1);
    assert!(page.moves[0].reversal);
    assert_eq!(page.moves[0].doc_number, Some(1));
    assert!(page.next.is_some());

    let saved = fx
        .db
        .crm_get_stock_doc(fx.community, fx.space, doc)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.void.as_ref().unwrap().reason, "ошибка");
    assert_eq!(saved.lines.unwrap()[0].cost_minor, Some(20_000));
    let low = fx
        .db
        .crm_list_stock(
            fx.community,
            fx.space,
            &StockQuery {
                low_only: true,
                search: Some("шамп".into()),
                ..StockQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!((low.items.len(), low.low_count), (1, 1));
    let _ = haircut;
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn tech_cards_are_compare_and_swap() {
    let fx = fixture().await;
    let dye = product(&fx, "Краска", "commodity").await;
    let gloves = product(&fx, "Перчатки", "commodity").await;
    let coloring = product(&fx, "Окрашивание", "service").await;
    let write = |items: Vec<(Uuid, i64)>, expected: Option<[u8; 32]>, revision: [u8; 32]| {
        let fx = &fx;
        async move {
            let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
            let outcome = write_tech_card(
                &mut tx,
                fx.community,
                fx.space,
                coloring,
                &items,
                expected.as_ref().map(|r| r.as_slice()),
                &revision,
                &fx.actor,
            )
            .await
            .unwrap();
            tx.commit().await.unwrap();
            outcome
        }
    };
    assert_eq!(
        write(vec![(dye, 60_000)], None, [5; 32]).await,
        TechCardWriteOutcome::Written
    );
    assert_eq!(
        write(vec![], None, [6; 32]).await,
        TechCardWriteOutcome::AlreadyExists
    );
    assert_eq!(
        write(vec![], Some([9; 32]), [6; 32]).await,
        TechCardWriteOutcome::RevisionMismatch
    );
    assert_eq!(
        write(vec![(gloves, 1_000), (dye, 50_000)], Some([5; 32]), [6; 32]).await,
        TechCardWriteOutcome::Written
    );
    let cards = fx
        .db
        .crm_list_tech_cards(fx.community, fx.space)
        .await
        .unwrap();
    assert_eq!(cards.len(), 1);
    let items: Vec<_> = cards[0]
        .items
        .iter()
        .map(|i| (i.product_name.as_str(), i.qty_milli))
        .collect();
    assert_eq!(items, vec![("Перчатки", 1_000), ("Краска", 50_000)]);

    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let used = consumables_for_services(&mut tx, fx.community, &[coloring])
        .await
        .unwrap();
    assert_eq!(used.len(), 2);
    assert_eq!(used[0].product_id, gloves);
    tx.rollback().await.unwrap();
}
