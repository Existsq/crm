use super::*;
use crate::crm::{create_space, SpaceCreateOutcome};
use buzz_core::channel::{ChannelType, ChannelVisibility};
use serde_json::json;

struct Fixture {
    db: Db,
    community: CommunityId,
    space: Uuid,
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
        .ensure_configured_community(&format!("crm-cat-{}.example", Uuid::new_v4()))
        .await
        .expect("community")
        .id;
    let actor = vec![7_u8; 32];
    let space = Uuid::new_v4();
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
        &json!({"currency": "RUB", "timezone": "Europe/Moscow"}),
        &[1; 32],
        &actor,
        Uuid::new_v4(),
        "Склад",
    )
    .await
    .expect("space");
    assert_eq!(created, SpaceCreateOutcome::Created);
    tx.commit().await.expect("commit");
    Fixture {
        db,
        community,
        space,
        actor,
    }
}

struct Draft {
    name: &'static str,
    sku: Option<&'static str>,
    barcodes: Vec<String>,
    archived: bool,
}

fn draft(name: &'static str) -> Draft {
    Draft {
        name,
        sku: None,
        barcodes: Vec::new(),
        archived: false,
    }
}

async fn write(
    fx: &Fixture,
    product: Uuid,
    draft: &Draft,
    revision: u8,
    expected: Option<u8>,
) -> ProductWriteOutcome {
    let revision = [revision; 32];
    let expected = expected.map(|byte| [byte; 32]);
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_product(
        &mut tx,
        fx.community,
        ProductWrite {
            product_id: product,
            space_channel_id: fx.space,
            name: draft.name,
            sku: draft.sku,
            barcodes: &draft.barcodes,
            unit: "piece",
            price_minor: 12_345,
            vat: "vat22",
            payment_object: "commodity",
            category: Some("Напитки"),
            min_stock_milli: 2_000,
            requires_marking: false,
            duration_min: None,
            archived: draft.archived,
            revision_event_id: &revision,
            actor: &fx.actor,
        },
        expected.as_ref().map(|r| r.as_slice()),
    )
    .await;
    match outcome {
        Ok(ProductWriteOutcome::Written) => {
            tx.commit().await.unwrap();
            ProductWriteOutcome::Written
        }
        Ok(other) => {
            tx.rollback().await.unwrap();
            other
        }
        Err(error) => panic!("write_product failed: {error}"),
    }
}

async fn list(fx: &Fixture, query: ProductQuery) -> ProductPage {
    fx.db
        .crm_list_products(fx.community, fx.space, &query)
        .await
        .unwrap()
}

fn query(limit: i64) -> ProductQuery {
    ProductQuery {
        limit,
        ..ProductQuery::default()
    }
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn create_then_cas_update_replaces_fields_and_barcodes() {
    let fx = fixture().await;
    let id = Uuid::new_v4();
    let mut product = draft("Чай зелёный");
    product.sku = Some("TEA-1");
    product.barcodes = vec!["4006381333931".into(), "INT-1".into()];
    assert_eq!(
        write(&fx, id, &product, 10, None).await,
        ProductWriteOutcome::Written
    );

    let stored = fx
        .db
        .crm_get_product(fx.community, fx.space, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.name, "Чай зелёный");
    assert_eq!(stored.barcodes, vec!["4006381333931", "INT-1"]);
    assert_eq!(stored.revision_event_id, vec![10; 32]);
    assert_eq!(stored.price_minor, 12_345);
    assert_eq!(stored.min_stock_milli, 2_000);

    // A re-signed create for an existing id is not an update.
    assert_eq!(
        write(&fx, id, &product, 11, None).await,
        ProductWriteOutcome::AlreadyExists
    );
    // Stale revision leaves the row untouched.
    product.name = "Чай чёрный";
    assert_eq!(
        write(&fx, id, &product, 12, Some(99)).await,
        ProductWriteOutcome::RevisionMismatch
    );
    // Update of an unknown id.
    assert_eq!(
        write(&fx, Uuid::new_v4(), &product, 13, Some(10)).await,
        ProductWriteOutcome::Missing
    );

    product.barcodes = vec!["INT-2".into(), "INT-1".into()];
    assert_eq!(
        write(&fx, id, &product, 14, Some(10)).await,
        ProductWriteOutcome::Written
    );
    let stored = fx
        .db
        .crm_get_product(fx.community, fx.space, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.name, "Чай чёрный");
    assert_eq!(stored.barcodes, vec!["INT-2", "INT-1"]);
    assert_eq!(stored.revision_event_id, vec![14; 32]);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn sku_and_barcodes_are_unique_per_space_and_released_on_edit() {
    let fx = fixture().await;
    let first = Uuid::new_v4();
    let mut a = draft("Кофе");
    a.sku = Some("CF-1");
    a.barcodes = vec!["4006381333931".into()];
    assert_eq!(
        write(&fx, first, &a, 20, None).await,
        ProductWriteOutcome::Written
    );

    let second = Uuid::new_v4();
    let mut b = draft("Кофе 2");
    b.sku = Some("cf-1");
    assert_eq!(
        write(&fx, second, &b, 21, None).await,
        ProductWriteOutcome::SkuTaken(Some("cf-1".into())),
        "SKUs compare case-insensitively"
    );
    b.sku = Some("CF-2");
    b.barcodes = vec!["INT-9".into(), "4006381333931".into()];
    assert_eq!(
        write(&fx, second, &b, 22, None).await,
        ProductWriteOutcome::BarcodeTaken(Some("4006381333931".into()))
    );

    // Archived products keep their codes reserved.
    a.archived = true;
    assert_eq!(
        write(&fx, first, &a, 23, Some(20)).await,
        ProductWriteOutcome::Written
    );
    assert_eq!(
        write(&fx, second, &b, 24, None).await,
        ProductWriteOutcome::BarcodeTaken(Some("4006381333931".into()))
    );

    // Removing the barcode from the first product frees it.
    a.barcodes.clear();
    assert_eq!(
        write(&fx, first, &a, 25, Some(23)).await,
        ProductWriteOutcome::Written
    );
    assert_eq!(
        write(&fx, second, &b, 26, None).await,
        ProductWriteOutcome::Written
    );
    // Writing a product with its own SKU is not a conflict.
    assert_eq!(
        write(&fx, second, &b, 27, Some(26)).await,
        ProductWriteOutcome::Written
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn list_filters_search_and_paginates_by_name() {
    let fx = fixture().await;
    let names = ["банан", "Апельсин", "вишня", "Груша 50%", "груша_зелёная"];
    let mut ids = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let id = Uuid::new_v4();
        let mut product = draft(name);
        if index == 2 {
            product.sku = Some("CHERRY-7");
            product.barcodes = vec!["96385074".into()];
        }
        if index == 1 {
            product.archived = true;
        }
        assert_eq!(
            write(&fx, id, &product, 30 + index as u8, None).await,
            ProductWriteOutcome::Written
        );
        ids.push(id);
    }

    let page = list(&fx, query(50)).await;
    let listed: Vec<&str> = page.products.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(listed, vec!["банан", "вишня", "Груша 50%", "груша_зелёная"]);
    assert_eq!(page.next, None);

    let with_archived = list(
        &fx,
        ProductQuery {
            include_archived: true,
            ..query(50)
        },
    )
    .await;
    assert_eq!(with_archived.products[0].name, "Апельсин");
    assert!(with_archived.products[0].archived);

    // Keyset pages cover every product exactly once.
    let mut seen = Vec::new();
    let mut after = None;
    loop {
        let page = list(
            &fx,
            ProductQuery {
                after,
                include_archived: true,
                ..query(2)
            },
        )
        .await;
        seen.extend(page.products.iter().map(|p| p.product_id));
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert_eq!(seen.len(), names.len());
    let mut sorted = seen.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len());

    let search = |q: &str| ProductQuery {
        q: Some(q.into()),
        ..query(50)
    };
    let by_name = list(&fx, search("ГРУША")).await;
    assert_eq!(by_name.products.len(), 2);
    // LIKE wildcards in the query are literal.
    let literal = list(&fx, search("50%")).await;
    assert_eq!(literal.products.len(), 1);
    let underscore = list(&fx, search("_")).await;
    assert_eq!(underscore.products[0].name, "груша_зелёная");
    assert_eq!(underscore.products.len(), 1);
    // SKU prefix, case-insensitive; not a substring match.
    assert_eq!(list(&fx, search("cherry")).await.products.len(), 1);
    assert_eq!(list(&fx, search("RRY-7")).await.products.len(), 0);

    let by_barcode = list(
        &fx,
        ProductQuery {
            barcode: Some("96385074".into()),
            ..query(50)
        },
    )
    .await;
    assert_eq!(by_barcode.products.len(), 1);
    assert_eq!(by_barcode.products[0].product_id, ids[2]);
    assert_eq!(by_barcode.products[0].barcodes, vec!["96385074"]);
    let missing = list(
        &fx,
        ProductQuery {
            barcode: Some("0000".into()),
            ..query(50)
        },
    )
    .await;
    assert!(missing.products.is_empty());
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn page_size_is_clamped() {
    let fx = fixture().await;
    for index in 0..3u8 {
        write(&fx, Uuid::new_v4(), &draft("Товар"), 40 + index, None).await;
    }
    let page = list(&fx, query(0)).await;
    assert_eq!(page.products.len(), 1, "limit 0 clamps to 1");
    assert!(page.next.is_some());
    let page = list(&fx, query(10_000)).await;
    assert_eq!(page.products.len(), 3);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn products_are_space_and_community_scoped() {
    let fx = fixture().await;
    let other = fixture().await;
    let id = Uuid::new_v4();
    let mut product = draft("Сыр");
    product.sku = Some("CHEESE");
    product.barcodes = vec!["INT-77".into()];
    assert_eq!(
        write(&fx, id, &product, 50, None).await,
        ProductWriteOutcome::Written
    );
    // Same SKU and barcode are free in another community's space.
    assert_eq!(
        write(&other, Uuid::new_v4(), &product, 51, None).await,
        ProductWriteOutcome::Written
    );
    assert!(other
        .db
        .crm_get_product(other.community, other.space, id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(list(&other, query(50)).await.products.len(), 1);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn concurrent_creates_with_one_barcode_admit_exactly_one() {
    let fx = fixture().await;
    let mut tasks = Vec::new();
    for index in 0..6u8 {
        let db = fx.db.clone();
        let community = fx.community;
        let space = fx.space;
        let actor = fx.actor.clone();
        tasks.push(tokio::spawn(async move {
            let barcodes = vec!["4006381333931".to_string()];
            let revision = [60 + index; 32];
            let mut tx = db.begin_event_write_transaction().await.unwrap();
            let outcome = write_product(
                &mut tx,
                community,
                ProductWrite {
                    product_id: Uuid::new_v4(),
                    space_channel_id: space,
                    name: "Гонка",
                    sku: None,
                    barcodes: &barcodes,
                    unit: "piece",
                    price_minor: 100,
                    vat: "vat22",
                    payment_object: "commodity",
                    category: None,
                    min_stock_milli: 0,
                    requires_marking: false,
                    duration_min: None,
                    archived: false,
                    revision_event_id: &revision,
                    actor: &actor,
                },
                None,
            )
            .await
            .unwrap();
            if outcome == ProductWriteOutcome::Written {
                match tx.commit().await {
                    Ok(()) => outcome,
                    Err(_) => ProductWriteOutcome::BarcodeTaken(None),
                }
            } else {
                tx.rollback().await.unwrap();
                outcome
            }
        }));
    }
    let mut written = 0;
    for task in tasks {
        match task.await.unwrap() {
            ProductWriteOutcome::Written => written += 1,
            ProductWriteOutcome::BarcodeTaken(_) => {}
            other => panic!("unexpected outcome {other:?}"),
        }
    }
    assert_eq!(written, 1);
}
