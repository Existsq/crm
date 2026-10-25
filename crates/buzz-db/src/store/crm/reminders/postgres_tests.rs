use super::*;
use crate::crm::appointments::{
    write_appointment, AppointmentServiceWrite, AppointmentWrite, AppointmentWriteOutcome,
};
use crate::crm::catalog::{write_product, ProductWrite, ProductWriteOutcome};
use crate::crm::parties::{write_party, PartyWrite, PartyWriteOutcome};
use crate::crm::staff::{write_staff, StaffWrite, StaffWriteOutcome};
use crate::crm::{create_space, SpaceCreateOutcome};
use buzz_core::channel::{ChannelType, ChannelVisibility};
use chrono::{Duration, NaiveDateTime, Timelike};
use serde_json::json;

const GRACE_SECS: i64 = 7200;

struct Fixture {
    db: Db,
    community: CommunityId,
    space: Uuid,
    actor: Vec<u8>,
    service: Uuid,
    staff: Uuid,
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
        .ensure_configured_community(&format!("crm-remind-{}.example", Uuid::new_v4()))
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
    let service = Uuid::new_v4();
    let staff = Uuid::new_v4();
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
    let product = write_product(
        &mut tx,
        community,
        ProductWrite {
            product_id: service,
            space_channel_id: space,
            name: "Стрижка",
            sku: None,
            barcodes: &[],
            unit: "piece",
            price_minor: 150_000,
            vat: "none",
            payment_object: "service",
            category: None,
            min_stock_milli: 0,
            requires_marking: false,
            duration_min: Some(30),
            archived: false,
            revision_event_id: &[2; 32],
            actor: &actor,
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(product, ProductWriteOutcome::Written);
    let master = write_staff(
        &mut tx,
        community,
        StaffWrite {
            staff_id: staff,
            space_channel_id: space,
            name: "Олег",
            position: None,
            phone: None,
            service_ids: &[service],
            member_pubkey: None,
            archived: false,
            revision_event_id: &[3; 32],
            actor: &actor,
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(master, StaffWriteOutcome::Written);
    tx.commit().await.expect("commit");
    Fixture {
        db,
        community,
        space,
        actor,
        service,
        staff,
    }
}

async fn customer(fx: &Fixture, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_party(
        &mut tx,
        fx.community,
        PartyWrite {
            party_id: id,
            space_channel_id: fx.space,
            party_type: "customer",
            name,
            phone: None,
            phone_digits: None,
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

/// Moscow wall-clock time `hours` from now, on the 5-minute grid and early
/// enough in its day for a 30-minute visit.
async fn local_in(fx: &Fixture, hours: i64) -> NaiveDateTime {
    let now: NaiveDateTime =
        sqlx::query_scalar("SELECT (now() AT TIME ZONE 'Europe/Moscow')::timestamp")
            .fetch_one(&fx.db.pool)
            .await
            .unwrap();
    let mut at = now + Duration::hours(hours);
    at = at.with_second(0).unwrap().with_nanosecond(0).unwrap();
    at += Duration::minutes(i64::from((5 - at.minute() % 5) % 5));
    if at.hour() * 60 + at.minute() > 1400 {
        at += Duration::hours(1);
    }
    at
}

async fn book(
    fx: &Fixture,
    id: Uuid,
    at: NaiveDateTime,
    customer: Uuid,
    status: &str,
    revision: u8,
    expected: Option<u8>,
) {
    let services = [AppointmentServiceWrite {
        product_id: fx.service,
        duration_min: 30,
        price_minor: 150_000,
    }];
    let start = (at.hour() * 60 + at.minute()) as i32;
    let expected = expected.map(|byte| [byte; 32]);
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_appointment(
        &mut tx,
        fx.community,
        AppointmentWrite {
            appointment_id: id,
            space_channel_id: fx.space,
            staff_id: fx.staff,
            day: at.date(),
            start_min: start,
            end_min: start + 30,
            services: &services,
            customer_id: Some(customer),
            client_name: None,
            client_phone: None,
            status,
            holds_time: !matches!(status, "no_show" | "cancelled"),
            note: None,
            source: "staff",
            outside_hours: true,
            revision_event_id: &[revision; 32],
            actor: &fx.actor,
        },
        expected.as_ref().map(|r| r.as_slice()),
    )
    .await
    .unwrap();
    assert_eq!(outcome, AppointmentWriteOutcome::Written);
    tx.commit().await.unwrap();
}

async fn save_settings(
    fx: &Fixture,
    enabled: bool,
    revision: u8,
    expected: Option<u8>,
) -> ReminderSettingsOutcome {
    let settings = json!({
        "v": 1,
        "enabled": enabled,
        "rules": [
            {"trigger": "booked", "template": "{client}, вы записаны на {time}"},
            {"trigger": "before", "offset_min": 240, "template": "Через 4 часа: {services}"},
            {"trigger": "before", "offset_min": 120, "template": "Через 2 часа"}
        ],
        "salon_name": "Барбершоп"
    });
    let expected = expected.map(|byte| [byte; 32]);
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_reminder_settings(
        &mut tx,
        fx.community,
        ReminderSettingsWrite {
            space_channel_id: fx.space,
            settings: &settings,
            enabled,
            revision_event_id: &[revision; 32],
            actor: &fx.actor,
        },
        expected.as_ref().map(|r| r.as_slice()),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    outcome
}

async fn link(fx: &Fixture, customer: Uuid, chat_id: Option<i64>, opted_out: bool) {
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_client_telegram(
        &mut tx,
        fx.community,
        ClientTelegramWrite {
            space_channel_id: fx.space,
            customer_id: customer,
            chat_id,
            opted_out,
            revision_event_id: &[9; 32],
        },
    )
    .await
    .unwrap();
    assert_eq!(outcome, ClientTelegramOutcome::Written);
    tx.commit().await.unwrap();
}

async fn due(fx: &Fixture) -> Vec<ReminderCandidate> {
    due_reminders(&fx.db.pool, GRACE_SECS, 10_000)
        .await
        .unwrap()
        .into_iter()
        .filter(|c| c.community_id == *fx.community.as_uuid())
        .collect()
}

async fn plan_all(fx: &Fixture, candidates: &[ReminderCandidate]) {
    for candidate in candidates {
        let (status, reason) = if candidate.chat_id.is_some() && !candidate.opted_out {
            ("pending", None)
        } else {
            ("skipped", Some("no_telegram"))
        };
        let planned = PlannedDelivery {
            candidate,
            text: "текст",
            status,
            reason,
        };
        assert!(plan_delivery(&fx.db.pool, planned).await.unwrap());
        assert!(
            !plan_delivery(&fx.db.pool, planned).await.unwrap(),
            "planned twice"
        );
    }
}

async fn claim_mine(fx: &Fixture) -> Vec<ClaimedDelivery> {
    claim_due_deliveries(&fx.db.pool, 600, 10_000)
        .await
        .unwrap()
        .into_iter()
        .filter(|c| c.community_id == *fx.community.as_uuid())
        .collect()
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn settings_are_compare_and_swap() {
    use ReminderSettingsOutcome as Out;
    let fx = fixture().await;
    assert_eq!(save_settings(&fx, true, 1, Some(9)).await, Out::Missing);
    assert_eq!(save_settings(&fx, true, 1, None).await, Out::Written);
    assert_eq!(save_settings(&fx, true, 2, None).await, Out::AlreadyExists);
    assert_eq!(
        save_settings(&fx, false, 2, Some(9)).await,
        Out::RevisionMismatch
    );
    assert_eq!(save_settings(&fx, false, 2, Some(1)).await, Out::Written);
    let record = get_reminder_settings(&fx.db.pool, fx.community, fx.space)
        .await
        .unwrap()
        .unwrap();
    assert!(!record.enabled);
    assert_eq!(record.revision_event_id, vec![2; 32]);
    assert_eq!(record.settings["rules"][1]["offset_min"], json!(240));
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn due_rules_are_planned_once_and_rechecked_before_sending() {
    let fx = fixture().await;
    let anna = customer(&fx, "Анна").await;
    let pyotr = customer(&fx, "Пётр").await;
    link(&fx, pyotr, Some(4242), false).await;
    let at = local_in(&fx, 3).await;

    // Visit A, booked five hours ago: its 4-hour reminder came due an hour
    // ago; its booking confirmation is past the grace window.
    let a = Uuid::new_v4();
    book(&fx, a, at, anna, "booked", 10, None).await;
    sqlx::query("UPDATE crm_appointments SET created_at = now() - interval '5 hours' WHERE community_id = $1 AND appointment_id = $2")
        .bind(fx.community.as_uuid())
        .bind(a)
        .execute(&fx.db.pool)
        .await
        .unwrap();
    // Visit B, booked just now: only its confirmation is due; the 4-hour
    // reminder was due before it was booked and never fires.
    let b = Uuid::new_v4();
    book(
        &fx,
        b,
        at + Duration::hours(1),
        pyotr,
        "confirmed",
        20,
        None,
    )
    .await;

    assert!(
        due(&fx).await.is_empty(),
        "nothing is due while reminders are off"
    );
    assert_eq!(
        save_settings(&fx, true, 1, None).await,
        ReminderSettingsOutcome::Written
    );
    let mut candidates = due(&fx).await;
    candidates.sort_by(|x, y| x.rule_key.cmp(&y.rule_key));
    let found: Vec<_> = candidates
        .iter()
        .map(|c| (c.appointment_id, c.rule_key.as_str(), c.chat_id))
        .collect();
    assert_eq!(found, [(a, "before:240", None), (b, "booked", Some(4242))]);
    let reminder = &candidates[0];
    assert_eq!(
        reminder.slot_key,
        at.format("%Y-%m-%d %H:%M").to_string(),
        "slot key is the visit's local time"
    );
    assert_eq!(reminder.client_name, "Анна");
    assert_eq!(reminder.staff_name, "Олег");
    assert_eq!(reminder.salon_name.as_deref(), Some("Барбершоп"));
    assert_eq!(reminder.services[0]["name"], json!("Стрижка"));
    let visit_utc = reminder.visit_at;
    assert_eq!(reminder.due_at, visit_utc - Duration::minutes(240));
    assert_eq!(
        candidates[1].slot_key, "",
        "a confirmation is once per visit"
    );

    plan_all(&fx, &candidates).await;
    assert!(
        due(&fx).await.is_empty(),
        "planned rules are not planned again"
    );

    // Only the pending confirmation is claimed, with the live visit state.
    let claimed = claim_mine(&fx).await;
    assert_eq!(claimed.len(), 1);
    let confirmation = &claimed[0];
    assert_eq!(confirmation.attempts, 1);
    assert_eq!(confirmation.visit_status.as_deref(), Some("confirmed"));
    assert_eq!(confirmation.chat_id, Some(4242));
    assert!(confirmation.enabled);
    assert!(claim_mine(&fx).await.is_empty(), "a claimed row is leased");

    // A retry goes back to pending with its error; then it is sent.
    let retry = DeliveryFinish {
        attempt: 1,
        status: "pending",
        reason: None,
        transport: Some("telegram"),
        chat_id: Some(4242),
        provider_message_id: None,
        last_error: Some("Too Many Requests"),
        retry_at: Some(Utc::now() - Duration::seconds(1)),
    };
    assert!(finish_delivery(
        &fx.db.pool,
        confirmation.community_id,
        confirmation.delivery_id,
        retry
    )
    .await
    .unwrap());
    let again = claim_mine(&fx).await;
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].attempts, 2);
    assert!(
        !finish_delivery(
            &fx.db.pool,
            again[0].community_id,
            again[0].delivery_id,
            retry
        )
        .await
        .unwrap(),
        "a sender whose claim was taken over writes nothing"
    );
    let sent = DeliveryFinish {
        attempt: 2,
        status: "sent",
        reason: None,
        transport: Some("telegram"),
        chat_id: Some(4242),
        provider_message_id: Some(77),
        last_error: None,
        retry_at: None,
    };
    assert!(finish_delivery(
        &fx.db.pool,
        again[0].community_id,
        again[0].delivery_id,
        sent
    )
    .await
    .unwrap());
    assert!(
        !finish_delivery(
            &fx.db.pool,
            again[0].community_id,
            again[0].delivery_id,
            DeliveryFinish {
                attempt: 2,
                ..retry
            }
        )
        .await
        .unwrap(),
        "a final state is never overwritten"
    );

    let log = list_deliveries(
        &fx.db.pool,
        fx.community,
        fx.space,
        &DeliveryQuery {
            limit: 50,
            ..DeliveryQuery::default()
        },
    )
    .await
    .unwrap();
    let mut states: Vec<_> = log
        .iter()
        .map(|d| {
            (
                d.rule_key.as_str(),
                d.status.as_str(),
                d.reason.as_deref(),
                d.attempts,
            )
        })
        .collect();
    states.sort();
    assert_eq!(
        states,
        [
            ("before:240", "skipped", Some("no_telegram"), 0),
            ("booked", "sent", None, 2)
        ]
    );
    let by_visit = list_deliveries(
        &fx.db.pool,
        fx.community,
        fx.space,
        &DeliveryQuery {
            appointment_id: Some(b),
            limit: 50,
            ..DeliveryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(by_visit.len(), 1);
    assert!(by_visit[0].sent_at.is_some());

    // Moving visit A plans its reminder again for the new time.
    let moved = at + Duration::minutes(30);
    book(&fx, a, moved, anna, "booked", 11, Some(10)).await;
    let replanned = due(&fx).await;
    assert_eq!(replanned.len(), 1);
    assert_eq!(
        replanned[0].slot_key,
        moved.format("%Y-%m-%d %H:%M").to_string()
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_claim_sees_cancellations_and_opt_outs() {
    let fx = fixture().await;
    let pyotr = customer(&fx, "Пётр").await;
    link(&fx, pyotr, Some(99), false).await;
    let at = local_in(&fx, 3).await;
    let visit = Uuid::new_v4();
    book(&fx, visit, at, pyotr, "booked", 10, None).await;
    assert_eq!(
        save_settings(&fx, true, 1, None).await,
        ReminderSettingsOutcome::Written
    );
    let candidates = due(&fx).await;
    assert_eq!(candidates.len(), 1);
    plan_all(&fx, &candidates).await;

    book(&fx, visit, at, pyotr, "cancelled", 11, Some(10)).await;
    assert_eq!(
        set_chat_opted_out(&fx.db.pool, fx.community, 99, true)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        set_chat_opted_out(&fx.db.pool, fx.community, 99, true)
            .await
            .unwrap(),
        0
    );
    let claimed = claim_mine(&fx).await;
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].visit_status.as_deref(), Some("cancelled"));
    assert!(claimed[0].opted_out);
    let cancel = DeliveryFinish {
        attempt: claimed[0].attempts,
        status: "cancelled",
        reason: Some("visit_changed"),
        transport: None,
        chat_id: None,
        provider_message_id: None,
        last_error: None,
        retry_at: None,
    };
    assert!(finish_delivery(
        &fx.db.pool,
        claimed[0].community_id,
        claimed[0].delivery_id,
        cancel
    )
    .await
    .unwrap());

    // Restoring the visit plans its confirmation afresh over the cancelled row.
    assert_eq!(
        set_chat_opted_out(&fx.db.pool, fx.community, 99, false)
            .await
            .unwrap(),
        1
    );
    book(&fx, visit, at, pyotr, "booked", 12, Some(11)).await;
    let again = due(&fx).await;
    assert_eq!(again.len(), 1);
    plan_all(&fx, &again).await;
    let claimed = claim_mine(&fx).await;
    assert_eq!(claimed.len(), 1);
    assert_eq!((claimed[0].attempts, claimed[0].opted_out), (1, false));
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn far_visits_get_their_booking_confirmation() {
    let fx = fixture().await;
    let pyotr = customer(&fx, "Пётр").await;
    link(&fx, pyotr, Some(98), false).await;
    let far = local_in(&fx, 24 * 30).await;
    let visit = Uuid::new_v4();
    book(&fx, visit, far, pyotr, "booked", 10, None).await;
    assert_eq!(
        save_settings(&fx, true, 1, None).await,
        ReminderSettingsOutcome::Written
    );
    let found: Vec<_> = due(&fx).await.into_iter().map(|c| c.rule_key).collect();
    assert_eq!(
        found,
        ["booked"],
        "only the confirmation is due a month ahead"
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn links_from_staff_and_bot() {
    let fx = fixture().await;
    let anna = customer(&fx, "Анна").await;
    assert_eq!(
        link_client_telegram_from_bot(&fx.db.pool, fx.community, Uuid::new_v4(), 5, None)
            .await
            .unwrap(),
        BotLinkOutcome::UnknownCustomer,
        "an unknown customer is never linked"
    );
    assert_eq!(
        link_client_telegram_from_bot(&fx.db.pool, fx.community, anna, 5, Some("anna"))
            .await
            .unwrap(),
        BotLinkOutcome::Linked("Анна".into())
    );
    assert_eq!(
        link_client_telegram_from_bot(&fx.db.pool, fx.community, anna, 6, Some("thief"))
            .await
            .unwrap(),
        BotLinkOutcome::OtherChat,
        "a forwarded invite never takes over a linked client"
    );
    let record = get_client_telegram(&fx.db.pool, fx.community, fx.space, anna)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            record.chat_id,
            record.username.as_deref(),
            record.linked_via.as_str()
        ),
        (Some(5), Some("anna"), "bot")
    );

    // Staff opting the client out keeps the bot's link details.
    link(&fx, anna, Some(5), true).await;
    let record = get_client_telegram(&fx.db.pool, fx.community, fx.space, anna)
        .await
        .unwrap()
        .unwrap();
    assert!(record.opted_out);
    assert_eq!(
        (record.username.as_deref(), record.linked_via.as_str()),
        (Some("anna"), "bot")
    );

    // Unlinking clears the chat and the bot's details.
    link(&fx, anna, None, false).await;
    let record = get_client_telegram(&fx.db.pool, fx.community, fx.space, anna)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (record.chat_id, record.username, record.linked_at),
        (None, None, None)
    );

    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let unknown = write_client_telegram(
        &mut tx,
        fx.community,
        ClientTelegramWrite {
            space_channel_id: fx.space,
            customer_id: Uuid::new_v4(),
            chat_id: Some(1),
            opted_out: false,
            revision_event_id: &[9; 32],
        },
    )
    .await
    .unwrap();
    assert_eq!(unknown, ClientTelegramOutcome::UnknownCustomer);
    tx.rollback().await.unwrap();
}
