use super::*;
use crate::crm::catalog::{write_product, ProductWrite, ProductWriteOutcome};
use crate::crm::parties::{write_party, PartyWrite, PartyWriteOutcome};
use crate::crm::staff::{
    write_staff, write_staff_days, StaffDayWrite, StaffWrite, StaffWriteOutcome,
};
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
        .ensure_configured_community(&format!("crm-appt-{}.example", Uuid::new_v4()))
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

async fn staff(fx: &Fixture, services: &[Uuid]) -> Uuid {
    let id = Uuid::new_v4();
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_staff(
        &mut tx,
        fx.community,
        StaffWrite {
            staff_id: id,
            space_channel_id: fx.space,
            name: "Olga",
            position: None,
            phone: None,
            service_ids: services,
            member_pubkey: None,
            archived: false,
            revision_event_id: &[3; 32],
            actor: &fx.actor,
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(outcome, StaffWriteOutcome::Written);
    // 10:00–14:00 and 15:00–20:00 on 2026-11-02.
    write_staff_days(
        &mut tx,
        fx.community,
        fx.space,
        id,
        &[StaffDayWrite {
            day: day(),
            intervals: json!([{"start": 600, "end": 840}, {"start": 900, "end": 1200}]),
        }],
        &[4; 32],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

async fn customer(fx: &Fixture) -> Uuid {
    let id = Uuid::new_v4();
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_party(
        &mut tx,
        fx.community,
        PartyWrite {
            party_id: id,
            space_channel_id: fx.space,
            party_type: "customer",
            name: "Pyotr",
            phone: Some("+7 900 000-00-01"),
            phone_digits: Some("79000000001"),
            email: None,
            inn: None,
            address: None,
            note: None,
            archived: false,
            revision_event_id: &[5; 32],
            actor: &fx.actor,
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(outcome, PartyWriteOutcome::Written);
    tx.commit().await.unwrap();
    id
}

fn day() -> NaiveDate {
    "2026-11-02".parse().unwrap()
}

struct Booking {
    id: Uuid,
    staff: Uuid,
    service: Uuid,
    start: i32,
    duration: i32,
    status: &'static str,
    outside_hours: bool,
    customer: Option<Uuid>,
}

fn booking(staff: Uuid, service: Uuid, start: i32) -> Booking {
    Booking {
        id: Uuid::new_v4(),
        staff,
        service,
        start,
        duration: 60,
        status: "booked",
        outside_hours: false,
        customer: None,
    }
}

async fn book(
    fx: &Fixture,
    b: &Booking,
    revision: u8,
    expected: Option<u8>,
) -> AppointmentWriteOutcome {
    let services = [AppointmentServiceWrite {
        product_id: b.service,
        duration_min: b.duration,
        price_minor: 150_000,
    }];
    let expected = expected.map(|byte| [byte; 32]);
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_appointment(
        &mut tx,
        fx.community,
        AppointmentWrite {
            appointment_id: b.id,
            space_channel_id: fx.space,
            staff_id: b.staff,
            day: day(),
            start_min: b.start,
            end_min: b.start + b.duration,
            services: &services,
            customer_id: b.customer,
            client_name: if b.customer.is_none() {
                Some("Walk-in")
            } else {
                None
            },
            client_phone: None,
            status: b.status,
            holds_time: !matches!(b.status, "no_show" | "cancelled"),
            note: None,
            source: "staff",
            outside_hours: b.outside_hours,
            revision_event_id: &[revision; 32],
            actor: &fx.actor,
        },
        expected.as_ref().map(|r| r.as_slice()),
    )
    .await
    .unwrap();
    if outcome == AppointmentWriteOutcome::Written {
        tx.commit().await.unwrap();
    } else {
        tx.rollback().await.unwrap();
    }
    outcome
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn bookings_respect_hours_and_never_overlap() {
    use AppointmentWriteOutcome as Out;
    let fx = fixture().await;
    let haircut = product(&fx, "service", Some(60)).await;
    let olga = staff(&fx, &[haircut]).await;

    let first = booking(olga, haircut, 600);
    assert_eq!(book(&fx, &first, 1, None).await, Out::Written);
    assert_eq!(book(&fx, &first, 1, None).await, Out::AlreadyExists);
    // 10:30 overlaps 10:00–11:00; 11:00 does not.
    assert_eq!(
        book(&fx, &booking(olga, haircut, 630), 2, None).await,
        Out::Overlap {
            start: 600,
            end: 660
        }
    );
    assert_eq!(
        book(&fx, &booking(olga, haircut, 660), 2, None).await,
        Out::Written
    );
    // 13:30–14:30 runs into the break; allowed only outside hours.
    let late = booking(olga, haircut, 810);
    assert_eq!(book(&fx, &late, 3, None).await, Out::OutsideHours);
    let late = Booking {
        outside_hours: true,
        ..late
    };
    assert_eq!(book(&fx, &late, 3, None).await, Out::Written);

    // Cancelling frees the slot for someone else.
    let cancelled = Booking {
        status: "cancelled",
        ..booking(olga, haircut, 600)
    };
    let cancelled = Booking {
        id: first.id,
        ..cancelled
    };
    assert_eq!(
        book(&fx, &cancelled, 4, Some(9)).await,
        Out::RevisionMismatch
    );
    assert_eq!(book(&fx, &cancelled, 4, Some(1)).await, Out::Written);
    assert_eq!(
        book(
            &fx,
            &Booking {
                duration: 45,
                ..booking(olga, haircut, 605)
            },
            5,
            None
        )
        .await,
        Out::Written
    );

    let from = Some(day());
    let list = fx
        .db
        .crm_list_appointments(
            fx.community,
            fx.space,
            &AppointmentQuery {
                from,
                to: from,
                staff_id: Some(olga),
                customer_id: None,
            },
        )
        .await
        .unwrap();
    let shown: Vec<_> = list
        .iter()
        .map(|a| (a.start_min, a.status.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            (600, "cancelled"),
            (605, "booked"),
            (660, "booked"),
            (810, "booked")
        ]
    );
    assert_eq!(list[1].services[0]["name"], json!("Haircut"));
    assert_eq!(list[1].total_minor, 150_000);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn bookings_check_staff_services_and_customers() {
    use AppointmentWriteOutcome as Out;
    let fx = fixture().await;
    let haircut = product(&fx, "service", Some(60)).await;
    let coloring = product(&fx, "service", Some(90)).await;
    let goods = product(&fx, "commodity", None).await;
    let olga = staff(&fx, &[haircut]).await;
    let pyotr = customer(&fx).await;

    assert_eq!(
        book(&fx, &booking(Uuid::new_v4(), haircut, 600), 1, None).await,
        Out::UnknownStaff
    );
    assert_eq!(
        book(&fx, &booking(olga, goods, 600), 1, None).await,
        Out::UnknownService(goods)
    );
    assert_eq!(
        book(&fx, &booking(olga, coloring, 600), 1, None).await,
        Out::ServiceNotOffered(coloring)
    );
    let stranger = Booking {
        customer: Some(Uuid::new_v4()),
        ..booking(olga, haircut, 600)
    };
    assert_eq!(book(&fx, &stranger, 1, None).await, Out::UnknownCustomer);
    let known = Booking {
        customer: Some(pyotr),
        ..booking(olga, haircut, 600)
    };
    assert_eq!(book(&fx, &known, 1, None).await, Out::Written);
    let record = fx
        .db
        .crm_get_appointment(fx.community, fx.space, known.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.customer_name.as_deref(), Some("Pyotr"));
    let history = fx
        .db
        .crm_list_appointments(
            fx.community,
            fx.space,
            &AppointmentQuery {
                customer_id: Some(pyotr),
                ..AppointmentQuery::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(history.len(), 1);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn slot_inputs_list_capable_staff_and_busy_time() {
    let fx = fixture().await;
    let haircut = product(&fx, "service", Some(45)).await;
    let coloring = product(&fx, "service", Some(90)).await;
    let olga = staff(&fx, &[haircut, coloring]).await;
    let anna = staff(&fx, &[haircut]).await;
    assert_eq!(
        book(&fx, &booking(olga, haircut, 600), 1, None).await,
        AppointmentWriteOutcome::Written
    );
    let inputs = fx
        .db
        .crm_slot_inputs(fx.community, fx.space, day(), &[coloring, haircut], None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        inputs.staff.iter().map(|s| s.0).collect::<Vec<_>>(),
        vec![olga]
    );
    assert_eq!(inputs.staff[0].2.as_array().unwrap().len(), 2);
    assert_eq!(inputs.busy, vec![(olga, 600, 660)]);
    assert_eq!(
        inputs
            .services
            .iter()
            .map(|s| (s.0, s.2))
            .collect::<Vec<_>>(),
        vec![(coloring, Some(90)), (haircut, Some(45))]
    );
    let both = fx
        .db
        .crm_slot_inputs(fx.community, fx.space, day(), &[haircut], None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(both.staff.len(), 2);
    assert!(both.staff.iter().any(|s| s.0 == anna));
}
