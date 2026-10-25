use super::*;
use crate::crm::parties::{write_party, PartyWrite, PartyWriteOutcome};
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
        .ensure_configured_community(&format!("crm-booking-{}.example", Uuid::new_v4()))
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

async fn save(
    fx: &Fixture,
    enabled: bool,
    revision: u8,
    expected: Option<u8>,
) -> OnlineBookingWriteOutcome {
    let expected = expected.map(|byte| [byte; 32]);
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let outcome = write_online_booking(
        &mut tx,
        fx.community,
        fx.space,
        enabled,
        &json!({"v": 1, "enabled": enabled, "title": "Борода"}),
        &[revision; 32],
        &fx.actor,
        expected.as_ref().map(|r| r.as_slice()),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    outcome
}

const LIMITS: CodeLimits = CodeLimits {
    resend_after_secs: 60,
    per_phone_per_day: 2,
    per_community_per_hour: 3,
};

async fn issue(fx: &Fixture, id: Uuid, key: &str) -> CodeIssueOutcome {
    fx.db
        .crm_issue_booking_code(
            fx.community,
            CodeIssue {
                verification_id: id,
                phone: "+7 900 000-00-01",
                phone_key: key,
                code_hash: &[9; 32],
                ttl_secs: 600,
            },
            LIMITS,
        )
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn settings_are_created_once_and_updated_by_revision() {
    use OnlineBookingWriteOutcome as Out;
    let fx = fixture().await;
    assert_eq!(
        fx.db
            .crm_get_online_booking(fx.community, fx.space)
            .await
            .unwrap(),
        None
    );
    assert_eq!(save(&fx, true, 1, Some(9)).await, Out::Missing);
    assert_eq!(save(&fx, true, 1, None).await, Out::Written);
    assert_eq!(save(&fx, true, 2, None).await, Out::AlreadyExists);
    assert_eq!(save(&fx, false, 2, Some(9)).await, Out::RevisionMismatch);
    assert_eq!(save(&fx, false, 2, Some(1)).await, Out::Written);
    let record = fx
        .db
        .crm_get_online_booking(fx.community, fx.space)
        .await
        .unwrap()
        .unwrap();
    assert!(!record.enabled);
    assert_eq!(record.revision_event_id, vec![2; 32]);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn codes_respect_resend_phone_and_community_limits() {
    let fx = fixture().await;
    assert_eq!(
        issue(&fx, Uuid::new_v4(), "79000000001").await,
        CodeIssueOutcome::Issued
    );
    let retry = issue(&fx, Uuid::new_v4(), "79000000001").await;
    assert!(matches!(retry, CodeIssueOutcome::RetryAfter(s) if (59..=60).contains(&s)));
    // Pretend the first code is old enough to resend.
    sqlx::query("UPDATE crm_booking_codes SET created_at = now() - interval '2 minutes', expires_at = now() + interval '1 minute' WHERE community_id = $1")
        .bind(fx.community.as_uuid())
        .execute(&fx.db.pool)
        .await
        .unwrap();
    assert_eq!(
        issue(&fx, Uuid::new_v4(), "79000000001").await,
        CodeIssueOutcome::Issued
    );
    sqlx::query("UPDATE crm_booking_codes SET created_at = now() - interval '2 minutes', expires_at = now() + interval '1 minute' WHERE community_id = $1")
        .bind(fx.community.as_uuid())
        .execute(&fx.db.pool)
        .await
        .unwrap();
    assert_eq!(
        issue(&fx, Uuid::new_v4(), "79000000001").await,
        CodeIssueOutcome::PhoneLimit
    );
    assert_eq!(
        issue(&fx, Uuid::new_v4(), "79000000002").await,
        CodeIssueOutcome::Issued
    );
    assert_eq!(
        issue(&fx, Uuid::new_v4(), "79000000003").await,
        CodeIssueOutcome::CommunityLimit
    );
    // A failed SMS is forgotten and frees its slot.
    let failed = Uuid::new_v4();
    sqlx::query(
        "DELETE FROM crm_booking_codes WHERE community_id = $1 AND phone_key = '79000000002'",
    )
    .bind(fx.community.as_uuid())
    .execute(&fx.db.pool)
    .await
    .unwrap();
    assert_eq!(
        issue(&fx, failed, "79000000004").await,
        CodeIssueOutcome::Issued
    );
    fx.db
        .crm_discard_booking_code(fx.community, failed)
        .await
        .unwrap();
    assert_eq!(
        issue(&fx, Uuid::new_v4(), "79000000005").await,
        CodeIssueOutcome::Issued
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn codes_lock_after_wrong_attempts_and_book_once() {
    let fx = fixture().await;
    let id = Uuid::new_v4();
    assert_eq!(
        issue(&fx, id, "79000000001").await,
        CodeIssueOutcome::Issued
    );
    let check = |hash: [u8; 32]| {
        let db = fx.db.clone();
        let community = fx.community;
        async move {
            db.crm_check_booking_code(community, id, &hash, 3)
                .await
                .unwrap()
        }
    };
    assert_eq!(
        fx.db
            .crm_check_booking_code(fx.community, Uuid::new_v4(), &[9; 32], 3)
            .await
            .unwrap(),
        CodeCheck::Unknown
    );
    assert_eq!(check([1; 32]).await, CodeCheck::Wrong { attempts_left: 2 });
    let verified = CodeCheck::Verified {
        phone: "+7 900 000-00-01".into(),
        phone_key: "79000000001".into(),
    };
    assert_eq!(check([9; 32]).await, verified);
    // Checking again (a retry after a taken slot) still passes.
    assert_eq!(check([9; 32]).await, verified);

    let appointment = Uuid::new_v4();
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    assert!(consume_code(&mut tx, fx.community, id, appointment)
        .await
        .unwrap());
    assert!(!consume_code(&mut tx, fx.community, id, appointment)
        .await
        .unwrap());
    tx.commit().await.unwrap();
    assert_eq!(check([9; 32]).await, CodeCheck::Used);

    let locked = Uuid::new_v4();
    sqlx::query("UPDATE crm_booking_codes SET created_at = now() - interval '2 minutes' WHERE community_id = $1")
        .bind(fx.community.as_uuid())
        .execute(&fx.db.pool)
        .await
        .unwrap();
    assert_eq!(
        issue(&fx, locked, "79000000001").await,
        CodeIssueOutcome::Issued
    );
    for left in [2, 1, 0] {
        assert_eq!(
            fx.db
                .crm_check_booking_code(fx.community, locked, &[1; 32], 3)
                .await
                .unwrap(),
            CodeCheck::Wrong {
                attempts_left: left
            }
        );
    }
    assert_eq!(
        fx.db
            .crm_check_booking_code(fx.community, locked, &[9; 32], 3)
            .await
            .unwrap(),
        CodeCheck::Locked
    );

    // An unverified code cannot book.
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    assert!(!consume_code(&mut tx, fx.community, locked, appointment)
        .await
        .unwrap());
    tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn customers_match_by_either_russian_spelling() {
    let fx = fixture().await;
    let mut tx = fx.db.begin_event_write_transaction().await.unwrap();
    let id = Uuid::new_v4();
    let outcome = write_party(
        &mut tx,
        fx.community,
        PartyWrite {
            party_id: id,
            space_channel_id: fx.space,
            party_type: "customer",
            name: "Pyotr",
            phone: Some("8 900 000-00-01"),
            phone_digits: Some("89000000001"),
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
    assert_eq!(
        customer_by_phone(&mut *tx, fx.community, fx.space, "79000000001")
            .await
            .unwrap(),
        Some(id)
    );
    assert_eq!(
        customer_by_phone(&mut *tx, fx.community, fx.space, "79000000002")
            .await
            .unwrap(),
        None
    );
    tx.rollback().await.unwrap();
}
