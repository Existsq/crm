//! Parties, sales and analytics against a real Postgres.

use super::*;
use crate::crm::analytics::{sales_analytics, AnalyticsPeriod};
use crate::crm::catalog::{write_product, ProductWrite, ProductWriteOutcome};
use crate::crm::parties::{
    list_parties, party_for_sale, write_party, PartyPage, PartyQuery, PartyRole, PartyWrite,
    PartyWriteOutcome,
};
use crate::crm::{create_space, SpaceCreateOutcome};
use buzz_core::channel::{ChannelType, ChannelVisibility};
use chrono::TimeZone as _;
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
        .ensure_configured_community(&format!("crm-sales-{}.example", Uuid::new_v4()))
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
    let warehouse = Uuid::new_v4();
    let mut tx = db.begin_event_write_transaction().await.expect("begin");
    let created = create_space(
        &mut tx,
        community,
        space,
        &json!({"currency": "RUB", "timezone": "Asia/Yekaterinburg"}),
        &[1; 32],
        &actor,
        warehouse,
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
        warehouse,
        actor,
    }
}

fn event_id() -> Vec<u8> {
    Uuid::new_v4()
        .as_bytes()
        .iter()
        .chain(Uuid::new_v4().as_bytes())
        .copied()
        .collect()
}

async fn product(fx: &Fixture, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    let revision = event_id();
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
            price_minor: 10_000,
            vat: "vat22",
            payment_object: "commodity",
            category: None,
            min_stock_milli: 0,
            requires_marking: false,
            archived: false,
            revision_event_id: &revision,
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

struct PartyDraft {
    party_type: &'static str,
    name: &'static str,
    phone: Option<&'static str>,
    inn: Option<&'static str>,
    archived: bool,
}

fn party_draft(name: &'static str) -> PartyDraft {
    PartyDraft {
        party_type: "customer",
        name,
        phone: None,
        inn: None,
        archived: false,
    }
}

async fn write(
    fx: &Fixture,
    id: Uuid,
    draft: &PartyDraft,
    revision: &[u8],
    expected: Option<&[u8]>,
) -> PartyWriteOutcome {
    let digits = draft
        .phone
        .map(|p| p.chars().filter(char::is_ascii_digit).collect::<String>());
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_party(
        &mut tx,
        fx.community,
        PartyWrite {
            party_id: id,
            space_channel_id: fx.space,
            party_type: draft.party_type,
            name: draft.name,
            phone: draft.phone,
            phone_digits: digits.as_deref(),
            email: None,
            inn: draft.inn,
            address: None,
            note: None,
            archived: draft.archived,
            revision_event_id: revision,
            actor: &fx.actor,
        },
        expected,
    )
    .await
    .unwrap();
    if outcome == PartyWriteOutcome::Written {
        tx.commit().await.unwrap();
    } else {
        tx.rollback().await.unwrap();
    }
    outcome
}

async fn party(fx: &Fixture, name: &'static str) -> Uuid {
    let id = Uuid::new_v4();
    assert_eq!(
        write(fx, id, &party_draft(name), &event_id(), None).await,
        PartyWriteOutcome::Written
    );
    id
}

/// Insert a sale of `(product, qty units, price)` lines at `sold_at`.
async fn sell(
    fx: &Fixture,
    customer: Option<Uuid>,
    payment: &str,
    sold_at: DateTime<Utc>,
    lines: &[(Uuid, i64, i64)],
) -> (Uuid, i64) {
    let sale_id = Uuid::new_v4();
    let event = event_id();
    let inserts: Vec<SaleLineInsert<'_>> = lines
        .iter()
        .map(|&(product_id, units, price)| SaleLineInsert {
            product_id,
            product_name: "snapshot",
            unit: "piece",
            vat: "vat22",
            qty_milli: units * 1000,
            price_minor: price,
            discount_minor: 0,
            amount_minor: units * price,
        })
        .collect();
    let total = inserts.iter().map(|line| line.amount_minor).sum();
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let number = insert_sale(
        &mut tx,
        fx.community,
        &SaleInsert {
            sale_id,
            space_channel_id: fx.space,
            warehouse_id: fx.warehouse,
            customer_id: customer,
            payment,
            total_minor: total,
            discount_minor: 0,
            note: None,
            sold_at,
            event_id: &event,
            actor: &fx.actor,
            lines: &inserts,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    (sale_id, number)
}

/// `hh:00` local time in Yekaterinburg (UTC+5) on 2026-10-`day`.
fn local(day: u32, hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap() - chrono::Duration::hours(5)
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn parties_cas_update_and_unique_inn() {
    let fx = fixture().await;
    let id = Uuid::new_v4();
    let mut draft = party_draft("ООО Ромашка");
    draft.party_type = "supplier";
    draft.inn = Some("7707083893");
    let first = event_id();
    assert_eq!(
        write(&fx, id, &draft, &first, None).await,
        PartyWriteOutcome::Written
    );
    assert_eq!(
        write(&fx, id, &draft, &event_id(), None).await,
        PartyWriteOutcome::AlreadyExists
    );
    assert_eq!(
        write(&fx, id, &draft, &event_id(), Some(&[9; 32])).await,
        PartyWriteOutcome::RevisionMismatch
    );
    assert_eq!(
        write(&fx, Uuid::new_v4(), &draft, &event_id(), Some(&first)).await,
        PartyWriteOutcome::Missing
    );

    // A second party cannot take the INN, archived or not.
    let other = Uuid::new_v4();
    let mut copy = party_draft("ИП Другой");
    copy.inn = Some("7707083893");
    assert_eq!(
        write(&fx, other, &copy, &event_id(), None).await,
        PartyWriteOutcome::InnTaken(Some("7707083893".into()))
    );
    draft.archived = true;
    let second = event_id();
    assert_eq!(
        write(&fx, id, &draft, &second, Some(&first)).await,
        PartyWriteOutcome::Written
    );
    assert!(matches!(
        write(&fx, other, &copy, &event_id(), None).await,
        PartyWriteOutcome::InnTaken(_)
    ));

    let stored = fx
        .db
        .crm_get_party(fx.community, fx.space, id)
        .await
        .unwrap()
        .unwrap();
    assert!(stored.archived);
    assert_eq!(stored.revision_event_id, second);
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let facts = party_for_sale(&mut tx, fx.community, fx.space, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (facts.party_type.as_str(), facts.archived),
        ("supplier", true)
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn party_search_filters_and_pages() {
    let fx = fixture().await;
    let mut maria = party_draft("Мария Иванова");
    maria.phone = Some("+7 (912) 000-11-22");
    let maria_id = Uuid::new_v4();
    write(&fx, maria_id, &maria, &event_id(), None).await;
    let mut supplier = party_draft("Молокозавод");
    supplier.party_type = "supplier";
    supplier.inn = Some("7736207543");
    write(&fx, Uuid::new_v4(), &supplier, &event_id(), None).await;
    let mut both = party_draft("Алексей");
    both.party_type = "both";
    write(&fx, Uuid::new_v4(), &both, &event_id(), None).await;

    let search = |q: &str, role: Option<PartyRole>| PartyQuery {
        q: Some(q.into()),
        role,
        limit: 50,
        ..PartyQuery::default()
    };
    let names = |page: PartyPage| page.parties.into_iter().map(|p| p.name).collect::<Vec<_>>();
    let pool = &fx.db.pool;
    assert_eq!(
        names(
            list_parties(pool, fx.community, fx.space, &search("иванов", None))
                .await
                .unwrap()
        ),
        vec!["Мария Иванова"]
    );
    assert_eq!(
        names(
            list_parties(pool, fx.community, fx.space, &search("000-11", None))
                .await
                .unwrap()
        ),
        vec!["Мария Иванова"]
    );
    assert_eq!(
        names(
            list_parties(pool, fx.community, fx.space, &search("773620", None))
                .await
                .unwrap()
        ),
        vec!["Молокозавод"]
    );
    let customers = PartyQuery {
        role: Some(PartyRole::Customers),
        limit: 50,
        ..PartyQuery::default()
    };
    assert_eq!(
        names(
            list_parties(pool, fx.community, fx.space, &customers)
                .await
                .unwrap()
        ),
        vec!["Алексей", "Мария Иванова"]
    );
    let suppliers = PartyQuery {
        role: Some(PartyRole::Suppliers),
        limit: 50,
        ..PartyQuery::default()
    };
    assert_eq!(
        names(
            list_parties(pool, fx.community, fx.space, &suppliers)
                .await
                .unwrap()
        ),
        vec!["Алексей", "Молокозавод"]
    );

    let first = list_parties(
        pool,
        fx.community,
        fx.space,
        &PartyQuery {
            limit: 2,
            ..PartyQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(first.parties.len(), 2);
    let rest = list_parties(
        pool,
        fx.community,
        fx.space,
        &PartyQuery {
            limit: 2,
            after: first.next,
            ..PartyQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(names(rest.clone()), vec!["Молокозавод"]);
    assert_eq!(rest.next, None);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn sales_are_numbered_listed_voided_and_counted_for_the_customer() {
    let fx = fixture().await;
    let kefir = product(&fx, "Кефир").await;
    let maria = party(&fx, "Мария").await;
    let (first, n1) = sell(
        &fx,
        Some(maria),
        "cash",
        local(20, 10),
        &[(kefir, 2, 9_000)],
    )
    .await;
    let (_, n2) = sell(&fx, None, "card", local(20, 11), &[(kefir, 1, 9_000)]).await;
    assert_eq!((n1, n2), (1, 2));

    let sale = fx
        .db
        .crm_get_sale(fx.community, fx.space, first)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(sale.total_minor, 18_000);
    assert_eq!(sale.customer_name.as_deref(), Some("Мария"));
    assert_eq!(sale.warehouse_name, "Склад");
    let lines = sale.lines.unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(
        (lines[0].qty_milli, lines[0].product_name.as_str()),
        (2_000, "snapshot")
    );

    let stats = fx
        .db
        .crm_get_party(fx.community, fx.space, maria)
        .await
        .unwrap()
        .unwrap()
        .purchases;
    assert_eq!((stats.sales_count, stats.revenue_minor), (1, 18_000));

    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let voided = void_sale(
        &mut tx,
        fx.community,
        fx.space,
        first,
        &[5; 32],
        "ошибка",
        &fx.actor,
    )
    .await
    .unwrap();
    assert_eq!(voided, SaleVoidOutcome::Voided);
    tx.commit().await.unwrap();
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    assert_eq!(
        void_sale(
            &mut tx,
            fx.community,
            fx.space,
            first,
            &[6; 32],
            "ещё раз",
            &fx.actor
        )
        .await
        .unwrap(),
        SaleVoidOutcome::AlreadyVoided
    );
    assert_eq!(
        void_sale(
            &mut tx,
            fx.community,
            fx.space,
            Uuid::new_v4(),
            &[6; 32],
            "x",
            &fx.actor
        )
        .await
        .unwrap(),
        SaleVoidOutcome::Missing
    );
    tx.rollback().await.unwrap();

    let all = list_sales(
        &fx.db.pool,
        fx.community,
        fx.space,
        &SaleQuery {
            include_voided: true,
            limit: 1,
            ..SaleQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(all.sales[0].number, 2);
    assert_eq!(all.next, Some(2));
    let older = list_sales(
        &fx.db.pool,
        fx.community,
        fx.space,
        &SaleQuery {
            include_voided: true,
            before: all.next,
            limit: 10,
            ..SaleQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(older.sales.len(), 1);
    assert_eq!(older.sales[0].void.as_ref().unwrap().reason, "ошибка");
    let live = list_sales(
        &fx.db.pool,
        fx.community,
        fx.space,
        &SaleQuery {
            limit: 10,
            ..SaleQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(live.sales.len(), 1);

    // A voided sale no longer counts toward the customer's purchases.
    let stats = fx
        .db
        .crm_get_party(fx.community, fx.space, maria)
        .await
        .unwrap()
        .unwrap()
        .purchases;
    assert_eq!((stats.sales_count, stats.revenue_minor), (0, 0));
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn analytics_use_space_days_and_skip_voided_sales() {
    let fx = fixture().await;
    let kefir = product(&fx, "Кефир").await;
    let tea = product(&fx, "Чай").await;
    let maria = party(&fx, "Мария").await;
    let ivan = party(&fx, "Иван").await;

    // Maria bought once before the period: she is a returning buyer.
    sell(&fx, Some(maria), "cash", local(9, 12), &[(tea, 1, 5_000)]).await;
    // 00:30 local on the 10th is still the 9th in UTC: it belongs to the 10th.
    sell(
        &fx,
        Some(maria),
        "cash",
        local(10, 0) + chrono::Duration::minutes(30),
        &[(kefir, 2, 9_000)],
    )
    .await;
    sell(
        &fx,
        Some(maria),
        "card",
        local(11, 18),
        &[(tea, 1, 5_000), (kefir, 1, 9_000)],
    )
    .await;
    sell(&fx, Some(ivan), "card", local(11, 18), &[(kefir, 3, 9_000)]).await;
    sell(&fx, None, "cash", local(12, 9), &[(tea, 2, 5_000)]).await;
    let (voided, _) = sell(
        &fx,
        Some(ivan),
        "transfer",
        local(12, 9),
        &[(tea, 10, 5_000)],
    )
    .await;
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    void_sale(
        &mut tx,
        fx.community,
        fx.space,
        voided,
        &[8; 32],
        "возврат",
        &fx.actor,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let period = AnalyticsPeriod {
        from: "2026-10-10".parse().unwrap(),
        to: "2026-10-12".parse().unwrap(),
        timezone: "Asia/Yekaterinburg".into(),
    };
    let analytics = sales_analytics(&fx.db.pool, fx.community, fx.space, &period)
        .await
        .unwrap();
    // 18 000 + 14 000 + 27 000 + 10 000.
    assert_eq!(analytics.totals.revenue_minor, 69_000);
    assert_eq!(analytics.totals.sales_count, 4);
    assert_eq!(analytics.totals.avg_check_minor, 17_250);
    let days: Vec<(String, i64)> = analytics
        .by_day
        .iter()
        .map(|b| (b.key.to_string(), b.revenue_minor))
        .collect();
    assert_eq!(
        days,
        vec![
            ("2026-10-10".to_string(), 18_000),
            ("2026-10-11".to_string(), 41_000),
            ("2026-10-12".to_string(), 10_000)
        ]
    );
    assert_eq!(
        analytics.by_hour.iter().map(|b| b.key).collect::<Vec<_>>(),
        vec![0, 9, 18]
    );
    assert_eq!(analytics.by_payment[0].key, "card");
    assert_eq!(analytics.by_payment[0].revenue_minor, 41_000);
    assert_eq!(analytics.top_products[0].name, "Кефир");
    assert_eq!(analytics.top_products[0].qty_milli, 6_000);
    assert_eq!(analytics.top_products[0].revenue_minor, 54_000);
    assert_eq!(analytics.top_customers[0].name, "Мария");
    assert_eq!(analytics.top_customers[0].sales_count, 2);
    assert_eq!(analytics.customers.buyers_count, 2);
    assert_eq!(analytics.customers.repeat_buyers_count, 1);
    // Ivan's first (non-voided) sale is in the period; Maria's is not.
    assert_eq!(analytics.customers.new_buyers_count, 1);
    assert_eq!(analytics.customers.anonymous_revenue_minor, 10_000);
    assert_eq!(analytics.customers.identified_revenue_minor, 59_000);

    let empty = AnalyticsPeriod {
        from: "2026-01-01".parse().unwrap(),
        to: "2026-01-31".parse().unwrap(),
        timezone: "Asia/Yekaterinburg".into(),
    };
    let none = sales_analytics(&fx.db.pool, fx.community, fx.space, &empty)
        .await
        .unwrap();
    assert_eq!(none.totals.sales_count, 0);
    assert_eq!(none.totals.avg_check_minor, 0);
    assert!(none.by_day.is_empty() && none.top_products.is_empty());
}
