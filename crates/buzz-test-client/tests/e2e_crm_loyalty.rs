//! End-to-end tests for the loyalty programme (kinds 42170, 42171,
//! `bonus_minor` on sales, `GET /crm/loyalty`).
//!
//! # Running
//!
//! ```text
//! RELAY_URL=ws://localhost:3000 \
//! DATABASE_URL=postgres://buzz:buzz_dev@localhost:5432/buzz \
//!   cargo test -p buzz-test-client --test e2e_crm_loyalty -- --ignored --test-threads=1
//! ```

mod crm_support;

use crm_support::*;
use nostr::Keys;
use serde_json::{json, Value};
use uuid::Uuid;

async fn bonuses(keys: &Keys, customer: Uuid) -> Value {
    let (status, body) = get(Some(keys), &format!("/crm/loyalty?customer_id={customer}")).await;
    assert_eq!(status, 200, "{body}");
    body
}

#[tokio::test]
#[ignore = "requires a running relay"]
async fn sales_earn_and_spend_bonuses_and_voids_revert_them() {
    let pool = pool().await;
    let (owner, channel, community) = fresh_space(&pool).await;
    let member = Keys::generate();
    add_member(&owner, channel, &member, "member").await;
    let warehouse: Uuid = get_space(&owner).await["warehouses"][0]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("default warehouse");
    let haircut = Uuid::new_v4();
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PRODUCT,
            channel,
            Some(haircut),
            &json!({"v": 1, "name": "Стрижка", "unit": "piece", "price_minor": 200_000,
                    "vat": "none", "payment_object": "service", "duration_min": 60}),
            None,
        ),
    )
    .await;
    let anna = Uuid::new_v4();
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PARTY,
            channel,
            Some(anna),
            &json!({"v": 1, "type": "customer", "name": "Анна"}),
            None,
        ),
    )
    .await;
    let sale = |discount: i64, bonus: Option<i64>| {
        let mut value = json!({
            "v": 1,
            "warehouse_id": warehouse,
            "customer_id": anna,
            "payment": "card",
            "lines": [{"product_id": haircut, "qty_milli": 1000, "price_minor": 200_000,
                       "discount_minor": discount}],
        });
        if let Some(bonus) = bonus {
            value["bonus_minor"] = json!(bonus);
        }
        value
    };

    // Off by default: reads show zeros, spending is refused, nothing is earned.
    let body = bonuses(&member, anna).await;
    assert_eq!(body["settings"]["enabled"], json!(false), "{body}");
    assert_eq!(body["customer"]["balance_minor"], json!(0), "{body}");
    reject(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_SALE,
            channel,
            Some(Uuid::new_v4()),
            &sale(100, Some(100)),
            None,
        ),
        "invalid: bonus_minor",
    )
    .await;

    // 5 % cashback, at most 30 % of a sale in bonuses; managers only.
    let settings = json!({"v": 1, "enabled": true, "cashback_bp": 500, "max_redeem_bp": 3000});
    reject(
        &member,
        &crm_event(&member, KIND_CRM_LOYALTY, channel, None, &settings, None),
        "restricted:",
    )
    .await;
    accept(
        &owner,
        &crm_event(&owner, KIND_CRM_LOYALTY, channel, None, &settings, None),
    )
    .await;

    let first = Uuid::new_v4();
    accept(
        &member,
        &crm_event(
            &member,
            KIND_CRM_SALE,
            channel,
            Some(first),
            &sale(0, None),
            None,
        ),
    )
    .await;
    let body = bonuses(&member, anna).await;
    assert_eq!(
        body["customer"]["balance_minor"],
        json!(10_000),
        "5 % of 2 000 ₽: {body}"
    );

    // A welcome bonus by hand; taking more than the balance is refused.
    let welcome = Uuid::new_v4();
    let adjust = |delta: i64| json!({"v": 1, "customer_id": anna, "delta_minor": delta, "reason": "приветственный бонус"});
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_BONUS_ADJUST,
            channel,
            Some(welcome),
            &adjust(70_000),
            None,
        ),
    )
    .await;
    reject(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_BONUS_ADJUST,
            channel,
            Some(welcome),
            &adjust(70_001),
            None,
        ),
        "conflict:",
    )
    .await;
    reject(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_BONUS_ADJUST,
            channel,
            Some(Uuid::new_v4()),
            &adjust(-80_001),
            None,
        ),
        "invalid: delta_minor",
    )
    .await;
    assert_eq!(
        bonuses(&member, anna).await["customer"]["balance_minor"],
        json!(80_000)
    );

    // Spend: at most 30 % of 2 000 ₽ = 600 ₽, and never above the balance.
    reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_SALE,
            channel,
            Some(Uuid::new_v4()),
            &sale(60_001, Some(60_001)),
            None,
        ),
        "invalid: bonus_minor",
    )
    .await;
    let second = Uuid::new_v4();
    accept(
        &member,
        &crm_event(
            &member,
            KIND_CRM_SALE,
            channel,
            Some(second),
            &sale(60_000, Some(60_000)),
            None,
        ),
    )
    .await;
    // 800 − 600 spent + 5 % of the 1 400 ₽ paid.
    let body = bonuses(&member, anna).await;
    assert_eq!(body["customer"]["balance_minor"], json!(27_000), "{body}");
    let sources: Vec<&str> = body["customer"]["moves"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["source"].as_str().unwrap())
        .collect();
    assert_eq!(
        sources,
        ["cashback", "redeem", "manual", "cashback"],
        "{body}"
    );

    // Voiding the second sale gives the spent bonuses back and takes the earned ones.
    accept(
        &owner,
        &sign(
            &owner,
            KIND_CRM_SALE_VOID,
            &json!({"v": 1, "reason": "ошибка"}).to_string(),
            vec![
                tag(&["h", &channel.to_string()]),
                tag(&["crm-id", &second.to_string()]),
            ],
        ),
    )
    .await;
    assert_eq!(
        bonuses(&member, anna).await["customer"]["balance_minor"],
        json!(80_000)
    );

    cleanup_space(&pool, community).await;
}
