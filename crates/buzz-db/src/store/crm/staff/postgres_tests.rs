use super::*;
use crate::crm::catalog::{write_product, ProductWrite, ProductWriteOutcome};
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
        .ensure_configured_community(&format!("crm-staff-{}.example", Uuid::new_v4()))
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
        actor,
    }
}

async fn product(fx: &Fixture, payment_object: &str, duration_min: Option<i32>) -> Uuid {
    let id = Uuid::new_v4();
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_product(
        &mut tx,
        fx.community,
        ProductWrite {
            product_id: id,
            space_channel_id: fx.space,
            name: "Haircut",
            sku: None,
            barcodes: &[],
            unit: "piece",
            price_minor: 150_000,
            vat: "none",
            payment_object,
            category: None,
            min_stock_milli: 0,
            requires_marking: false,
            duration_min,
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

async fn write(
    fx: &Fixture,
    staff_id: Uuid,
    name: &str,
    services: &[Uuid],
    archived: bool,
    revision: u8,
    expected: Option<u8>,
) -> StaffWriteOutcome {
    let revision = [revision; 32];
    let expected = expected.map(|byte| [byte; 32]);
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_staff(
        &mut tx,
        fx.community,
        StaffWrite {
            staff_id,
            space_channel_id: fx.space,
            name,
            position: Some("Barber"),
            phone: None,
            service_ids: services,
            member_pubkey: None,
            archived,
            revision_event_id: &revision,
            actor: &fx.actor,
        },
        expected.as_ref().map(|r| r.as_slice()),
    )
    .await
    .unwrap();
    if outcome == StaffWriteOutcome::Written {
        tx.commit().await.unwrap();
    } else {
        tx.rollback().await.unwrap();
    }
    outcome
}

async fn set_days(
    fx: &Fixture,
    staff_id: Uuid,
    days: &[(&str, serde_json::Value)],
) -> ScheduleWriteOutcome {
    let days: Vec<StaffDayWrite> = days
        .iter()
        .map(|(day, intervals)| StaffDayWrite {
            day: day.parse().unwrap(),
            intervals: intervals.clone(),
        })
        .collect();
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_staff_days(&mut tx, fx.community, fx.space, staff_id, &days, &[9; 32])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    outcome
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn staff_create_update_and_cas() {
    let fx = fixture().await;
    let haircut = product(&fx, "service", Some(45)).await;
    let staff = Uuid::new_v4();

    assert_eq!(
        write(&fx, staff, "Olga", &[haircut], false, 1, Some(0)).await,
        StaffWriteOutcome::Missing
    );
    assert_eq!(
        write(&fx, staff, "Olga", &[haircut], false, 1, None).await,
        StaffWriteOutcome::Written
    );
    assert_eq!(
        write(&fx, staff, "Olga", &[], false, 2, None).await,
        StaffWriteOutcome::AlreadyExists
    );
    assert_eq!(
        write(&fx, staff, "Olga K.", &[], false, 2, Some(5)).await,
        StaffWriteOutcome::RevisionMismatch
    );
    assert_eq!(
        write(&fx, staff, "Olga K.", &[haircut], false, 2, Some(1)).await,
        StaffWriteOutcome::Written
    );

    let record = fx
        .db
        .crm_get_staff(fx.community, fx.space, staff)
        .await
        .unwrap()
        .expect("staff");
    assert_eq!(record.name, "Olga K.");
    assert_eq!(record.service_ids, vec![haircut]);
    assert_eq!(record.revision_event_id, vec![2; 32]);

    let products = fx
        .db
        .crm_get_product(fx.community, fx.space, haircut)
        .await
        .unwrap()
        .expect("product");
    assert_eq!(products.duration_min, Some(45));
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn staff_services_must_be_bookable_products_of_the_space() {
    let fx = fixture().await;
    let goods = product(&fx, "commodity", None).await;
    let stranger = Uuid::new_v4();
    let staff = Uuid::new_v4();
    assert_eq!(
        write(&fx, staff, "Olga", &[goods], false, 1, None).await,
        StaffWriteOutcome::UnknownService(goods)
    );
    assert_eq!(
        write(&fx, staff, "Olga", &[stranger], false, 1, None).await,
        StaffWriteOutcome::UnknownService(stranger)
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn archived_staff_leave_the_default_list_and_take_no_schedule() {
    let fx = fixture().await;
    let olga = Uuid::new_v4();
    let anna = Uuid::new_v4();
    assert_eq!(
        write(&fx, olga, "olga", &[], false, 1, None).await,
        StaffWriteOutcome::Written
    );
    assert_eq!(
        write(&fx, anna, "Anna", &[], false, 2, None).await,
        StaffWriteOutcome::Written
    );
    assert_eq!(
        write(&fx, olga, "olga", &[], true, 3, Some(1)).await,
        StaffWriteOutcome::Written
    );
    let active = fx
        .db
        .crm_list_staff(fx.community, fx.space, false)
        .await
        .unwrap();
    assert_eq!(
        active.iter().map(|s| s.staff_id).collect::<Vec<_>>(),
        vec![anna]
    );
    let all = fx
        .db
        .crm_list_staff(fx.community, fx.space, true)
        .await
        .unwrap();
    assert_eq!(
        all.iter().map(|s| s.staff_id).collect::<Vec<_>>(),
        vec![anna, olga]
    );
    assert!(all[1].archived);

    let nine_to_six = json!([{"start": 540, "end": 1080}]);
    assert_eq!(
        set_days(&fx, olga, &[("2026-11-02", nine_to_six.clone())]).await,
        ScheduleWriteOutcome::ArchivedStaff
    );
    assert_eq!(
        set_days(&fx, Uuid::new_v4(), &[("2026-11-02", nine_to_six)]).await,
        ScheduleWriteOutcome::UnknownStaff
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn schedule_days_upsert_and_keep_unlisted_days() {
    let fx = fixture().await;
    let staff = Uuid::new_v4();
    assert_eq!(
        write(&fx, staff, "Olga", &[], false, 1, None).await,
        StaffWriteOutcome::Written
    );
    let day = json!([{"start": 540, "end": 1080}]);
    let short = json!([{"start": 600, "end": 720}]);
    assert_eq!(
        set_days(
            &fx,
            staff,
            &[("2026-11-02", day.clone()), ("2026-11-03", day.clone())]
        )
        .await,
        ScheduleWriteOutcome::Written
    );
    assert_eq!(
        set_days(
            &fx,
            staff,
            &[("2026-11-03", short.clone()), ("2026-11-04", json!([]))]
        )
        .await,
        ScheduleWriteOutcome::Written
    );
    let from = "2026-11-01".parse().unwrap();
    let to = "2026-11-30".parse().unwrap();
    let days = fx
        .db
        .crm_list_staff_days(fx.community, fx.space, from, to)
        .await
        .unwrap();
    let got: Vec<(String, serde_json::Value)> = days
        .into_iter()
        .map(|d| (d.day.to_string(), d.intervals))
        .collect();
    assert_eq!(
        got,
        vec![
            ("2026-11-02".to_owned(), day),
            ("2026-11-03".to_owned(), short),
            ("2026-11-04".to_owned(), json!([])),
        ]
    );
}
