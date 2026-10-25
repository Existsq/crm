//! End-to-end tests for visit reminders (kinds 42150, 42151,
//! `GET /crm/reminders*`, `POST /crm/telegram/webhook`) against a mock
//! Telegram Bot API.
//!
//! # Running
//!
//! The relay must point its bot at the mock this test serves and tick fast:
//!
//! ```text
//! BUZZ_CRM_TELEGRAM_BOT_TOKEN=e2e-token \
//! BUZZ_CRM_TELEGRAM_BOT_USERNAME=e2e_salon_bot \
//! BUZZ_CRM_TELEGRAM_WEBHOOK_SECRET=e2e-secret \
//! BUZZ_CRM_TELEGRAM_API_BASE=http://127.0.0.1:18765 \
//! BUZZ_CRM_REMINDER_INTERVAL_SECS=1 just relay
//!
//! RELAY_URL=ws://localhost:3000 \
//! DATABASE_URL=postgres://buzz:buzz_dev@localhost:5432/buzz \
//!   cargo test -p buzz-test-client --test e2e_crm_reminders -- --ignored --test-threads=1
//! ```

mod crm_support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crm_support::*;
use nostr::Keys;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

/// A chat the mock answers like a client who blocked the bot.
const BLOCKED_CHAT: i64 = 403;

fn mock_addr() -> String {
    std::env::var("TELEGRAM_MOCK_ADDR").unwrap_or_else(|_| "127.0.0.1:18765".into())
}

fn webhook_secret() -> String {
    std::env::var("TELEGRAM_WEBHOOK_SECRET").unwrap_or_else(|_| "e2e-secret".into())
}

/// `sendMessage` bodies the relay posted, in order.
type Outbox = Arc<Mutex<Vec<Value>>>;

/// Serve a minimal Bot API: every `sendMessage` is recorded and answered
/// `ok`, except to [`BLOCKED_CHAT`], which gets Telegram's 403.
async fn serve_mock_telegram() -> Outbox {
    let listener = TcpListener::bind(mock_addr())
        .await
        .expect("bind the mock Telegram address the relay was started with");
    let outbox: Outbox = Arc::default();
    let sink = Arc::clone(&outbox);
    tokio::spawn(async move {
        let mut next_id = 1_i64;
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut raw = Vec::new();
            let mut buf = [0_u8; 4096];
            let body = loop {
                let Ok(n) = socket.read(&mut buf).await else {
                    break None;
                };
                if n == 0 {
                    break None;
                }
                raw.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&raw);
                let Some(end) = text.find("\r\n\r\n") else {
                    continue;
                };
                let length = text[..end]
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                if raw.len() >= end + 4 + length {
                    break Some(raw[end + 4..end + 4 + length].to_vec());
                }
            };
            let Some(body) = body else { continue };
            let message: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let (status, reply) = if message["chat_id"] == json!(BLOCKED_CHAT) {
                (
                    "403 Forbidden",
                    json!({"ok": false, "error_code": 403, "description": "Forbidden: bot was blocked by the user"}),
                )
            } else {
                next_id += 1;
                (
                    "200 OK",
                    json!({"ok": true, "result": {"message_id": next_id}}),
                )
            };
            sink.lock().unwrap().push(message);
            let reply = reply.to_string();
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    outbox
}

/// Wait until a message to `chat` containing `needle` arrives.
async fn wait_for_message(outbox: &Outbox, chat: i64, needle: &str) -> String {
    for _ in 0..100 {
        let found = outbox
            .lock()
            .unwrap()
            .iter()
            .find(|m| {
                m["chat_id"] == json!(chat)
                    && m["text"].as_str().is_some_and(|t| t.contains(needle))
            })
            .and_then(|m| m["text"].as_str().map(str::to_owned));
        if let Some(text) = found {
            return text;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "no message to {chat} containing {needle:?}; got {:?}",
        outbox.lock().unwrap()
    );
}

/// Wait until the delivery log of `appointment` shows a final state.
async fn wait_for_delivery(keys: &Keys, appointment: Uuid) -> Value {
    for _ in 0..100 {
        let (status, body) = get(
            Some(keys),
            &format!("/crm/reminders/deliveries?appointment_id={appointment}"),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        if let Some(row) = body["deliveries"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["status"] != json!("pending")))
        {
            return row.clone();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("no final delivery for {appointment}");
}

async fn webhook(chat: i64, text: &str, secret: &str) -> u16 {
    http()
        .post(format!("{}/crm/telegram/webhook", relay_http_url()))
        .header("X-Telegram-Bot-Api-Secret-Token", secret)
        .json(&json!({
            "update_id": 1,
            "message": {"message_id": 1, "chat": {"id": chat, "type": "private"},
                        "from": {"id": chat, "username": "anna_tg"}, "text": text}
        }))
        .send()
        .await
        .expect("POST webhook")
        .status()
        .as_u16()
}

fn settings(enabled: bool) -> Value {
    json!({
        "v": 1,
        "enabled": enabled,
        "rules": [
            {"trigger": "booked", "template": "{client}, вы записаны в {salon} на {date} в {time}: {services}, мастер {staff}"},
            {"trigger": "before", "offset_min": 1440, "template": "Завтра в {time} ждём вас"}
        ],
        "salon_name": "Барбершоп «Борода»"
    })
}

/// A visit two days ahead, outside any working hours, booked for `customer`.
fn visit(staff: Uuid, service: Uuid, customer: Uuid, start: &str) -> Value {
    let day = (chrono::Utc::now().date_naive() + chrono::Days::new(2))
        .format("%Y-%m-%d")
        .to_string();
    json!({
        "v": 1,
        "staff_id": staff,
        "date": day,
        "start": start,
        "services": [{"product_id": service, "duration_min": 60, "price_minor": 150_000}],
        "customer_id": customer,
        "status": "booked",
        "outside_hours": true,
    })
}

#[tokio::test]
#[ignore = "requires a relay started with the mock Telegram bot"]
async fn clients_subscribe_in_telegram_and_get_their_reminders() {
    let outbox = serve_mock_telegram().await;
    let pool = pool().await;
    let (owner, channel, _community) = fresh_space(&pool).await;
    let member = Keys::generate();
    add_member(&owner, channel, &member, "member").await;

    let (status, body) = get(Some(&member), "/crm/reminders").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["settings"], Value::Null);
    assert_eq!(body["transport"]["mode"], json!("telegram"));
    assert_eq!(body["transport"]["bot_username"], json!("e2e_salon_bot"));

    // Only owners and admins change the rules; a typo in a template is caught.
    reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_REMINDER_SETTINGS,
            channel,
            None,
            &settings(true),
            None,
        ),
        "restricted:",
    )
    .await;
    let mut typo = settings(true);
    typo["rules"][0]["template"] = json!("Привет, {name}");
    reject(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_REMINDER_SETTINGS,
            channel,
            None,
            &typo,
            None,
        ),
        "invalid:",
    )
    .await;
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_REMINDER_SETTINGS,
            channel,
            None,
            &settings(true),
            None,
        ),
    )
    .await;
    let (_, body) = get(Some(&member), "/crm/reminders").await;
    assert_eq!(body["enabled"], json!(true));
    let revision = body["revision"].as_str().unwrap().to_owned();
    reject(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_REMINDER_SETTINGS,
            channel,
            None,
            &settings(false),
            None,
        ),
        "conflict:",
    )
    .await;

    // Catalog: one service, one master, two clients.
    let service = Uuid::new_v4();
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_PRODUCT,
            channel,
            Some(service),
            &json!({"v": 1, "name": "Стрижка", "unit": "piece", "price_minor": 150_000,
                    "vat": "none", "payment_object": "service", "duration_min": 60}),
            None,
        ),
    )
    .await;
    let staff = Uuid::new_v4();
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_STAFF,
            channel,
            Some(staff),
            &json!({"v": 1, "name": "Олег", "service_ids": [service], "archived": false}),
            None,
        ),
    )
    .await;
    let anna = Uuid::new_v4();
    let boris = Uuid::new_v4();
    for (id, name) in [(anna, "Анна"), (boris, "Борис")] {
        accept(
            &member,
            &crm_event(
                &member,
                KIND_CRM_PARTY,
                channel,
                Some(id),
                &json!({"v": 1, "type": "customer", "name": name}),
                None,
            ),
        )
        .await;
    }

    // Anna opens her invite link: the bot links her chat.
    let (status, link) = get(Some(&member), &format!("/crm/parties/{anna}/telegram")).await;
    assert_eq!(status, 200, "{link}");
    assert_eq!(link["linked"], json!(false));
    let code = link["start_code"].as_str().unwrap().to_owned();
    assert_eq!(
        link["invite_url"],
        json!(format!("https://t.me/e2e_salon_bot?start={code}"))
    );
    let anna_chat = 7_000_000_000_i64 + i64::from(rand::random::<u16>());
    assert_eq!(
        webhook(anna_chat, &format!("/start {code}"), "wrong").await,
        404
    );
    assert_eq!(
        webhook(anna_chat, "/start forged-code", &webhook_secret()).await,
        200
    );
    wait_for_message(&outbox, anna_chat, "недействительна").await;
    assert_eq!(
        webhook(anna_chat, &format!("/start {code}"), &webhook_secret()).await,
        200
    );
    wait_for_message(&outbox, anna_chat, "Готово, Анна").await;
    let (_, link) = get(Some(&member), &format!("/crm/parties/{anna}/telegram")).await;
    assert_eq!(link["linked"], json!(true));
    assert_eq!(link["chat_id"], json!(anna_chat));
    assert_eq!(link["username"], json!("anna_tg"));
    assert_eq!(link["linked_via"], json!("bot"));

    // Booking Anna sends her confirmation through the bot.
    let anna_visit = Uuid::new_v4();
    accept(
        &member,
        &crm_event(
            &member,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(anna_visit),
            &visit(staff, service, anna, "11:00"),
            None,
        ),
    )
    .await;
    let text = wait_for_message(&outbox, anna_chat, "вы записаны").await;
    assert!(
        text.starts_with("Анна, вы записаны в Барбершоп «Борода» на ")
            && text.ends_with("в 11:00: Стрижка, мастер Олег"),
        "{text}"
    );
    let delivery = wait_for_delivery(&member, anna_visit).await;
    assert_eq!(delivery["status"], json!("sent"), "{delivery}");
    assert_eq!(delivery["transport"], json!("telegram"));
    assert_eq!(delivery["rule_key"], json!("booked"));
    assert_eq!(delivery["text"], json!(text));

    // Boris has no chat: his confirmation is logged as skipped.
    let boris_visit = Uuid::new_v4();
    accept(
        &member,
        &crm_event(
            &member,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(boris_visit),
            &visit(staff, service, boris, "13:00"),
            None,
        ),
    )
    .await;
    let delivery = wait_for_delivery(&member, boris_visit).await;
    assert_eq!(
        (delivery["status"].clone(), delivery["reason"].clone()),
        (json!("skipped"), json!("no_telegram"))
    );

    // Staff link Boris to a chat that blocked the bot: the send fails as
    // blocked and Boris is opted out.
    accept(
        &member,
        &crm_event(
            &member,
            KIND_CRM_CLIENT_TELEGRAM,
            channel,
            None,
            &json!({"v": 1, "customer_id": boris, "chat_id": BLOCKED_CHAT}),
            None,
        ),
    )
    .await;
    let blocked_visit = Uuid::new_v4();
    accept(
        &member,
        &crm_event(
            &member,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(blocked_visit),
            &visit(staff, service, boris, "15:00"),
            None,
        ),
    )
    .await;
    let delivery = wait_for_delivery(&member, blocked_visit).await;
    assert_eq!(
        (delivery["status"].clone(), delivery["reason"].clone()),
        (json!("failed"), json!("blocked")),
        "{delivery}"
    );
    let (_, link) = get(Some(&member), &format!("/crm/parties/{boris}/telegram")).await;
    assert_eq!(link["opted_out"], json!(true));
    reject(
        &member,
        &crm_event(
            &member,
            KIND_CRM_CLIENT_TELEGRAM,
            channel,
            None,
            &json!({"v": 1, "customer_id": Uuid::new_v4(), "chat_id": 5}),
            None,
        ),
        "invalid:",
    )
    .await;

    // Anna sends /stop: her next booking is skipped as opted out.
    assert_eq!(webhook(anna_chat, "/stop", &webhook_secret()).await, 200);
    wait_for_message(&outbox, anna_chat, "отписались").await;
    let quiet_visit = Uuid::new_v4();
    accept(
        &member,
        &crm_event(
            &member,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(quiet_visit),
            &visit(staff, service, anna, "17:00"),
            None,
        ),
    )
    .await;
    let delivery = wait_for_delivery(&member, quiet_visit).await;
    assert_eq!(
        (delivery["status"].clone(), delivery["reason"].clone()),
        (json!("skipped"), json!("opted_out"))
    );

    // Turning reminders off stops planning.
    accept(
        &owner,
        &crm_event(
            &owner,
            KIND_CRM_REMINDER_SETTINGS,
            channel,
            None,
            &settings(false),
            Some(&revision),
        ),
    )
    .await;
    assert_eq!(webhook(anna_chat, "/resume", &webhook_secret()).await, 200);
    let silent_visit = Uuid::new_v4();
    accept(
        &member,
        &crm_event(
            &member,
            KIND_CRM_APPOINTMENT,
            channel,
            Some(silent_visit),
            &visit(staff, service, anna, "19:00"),
            None,
        ),
    )
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let (_, body) = get(
        Some(&member),
        &format!("/crm/reminders/deliveries?appointment_id={silent_visit}"),
    )
    .await;
    assert_eq!(body["deliveries"], json!([]), "{body}");
    let (_, body) = get(Some(&member), "/crm/reminders/deliveries").await;
    assert_eq!(body["deliveries"].as_array().unwrap().len(), 4, "{body}");
}
