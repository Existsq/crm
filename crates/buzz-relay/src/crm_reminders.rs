//! CRM visit reminders: the worker that plans and sends them, the Telegram
//! transport and the invite codes that link a client's chat.
//!
//! Each tick plans every rule that came due (one `crm_reminder_deliveries`
//! row per visit, rule and visit time, written with `ON CONFLICT DO NOTHING`
//! so pods never plan twice), then claims due rows with a lease, re-checks
//! them against the live visit and sends them. A transient Telegram failure
//! retries with backoff until [`MAX_ATTEMPTS`]; a crashed sender's rows come
//! back when their lease ends, so delivery is at least once.
//!
//! Without `BUZZ_CRM_TELEGRAM_BOT_TOKEN` the relay runs a sandbox transport:
//! messages are rendered, logged as sent with `transport = 'sandbox'` and
//! never leave the relay.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use chrono::{TimeDelta, Utc};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use tracing::{error, info, warn};
use uuid::Uuid;

use buzz_core::CommunityId;
use buzz_crm::reminder::{format_rub, format_visit_date, render, ReminderContext};
use buzz_db::crm::{ClaimedDelivery, DeliveryFinish, PlannedDelivery, ReminderCandidate};

use crate::state::AppState;

/// A rule is sent until this long after it came due; later it is skipped.
const GRACE_SECS: i64 = 3600;
/// Most rules planned per tick.
const PLAN_BATCH: i64 = 200;
/// Most messages sent per tick.
const SEND_BATCH: i64 = 50;
/// Timeout of one Telegram request.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a claimed batch is held: longer than the whole batch can take,
/// so no other pod re-sends a row this pod is still working through.
const LEASE_SECS: i64 = SEND_BATCH * 10 + 60;
/// How long into a lease this pod may still start a send.
const LEASE_SAFE_SEND: Duration = Duration::from_secs((LEASE_SECS - 60) as u64);
/// Attempts before a message is given up as failed.
pub const MAX_ATTEMPTS: i32 = 5;
/// Telegram's message limit, in characters.
const MAX_MESSAGE_CHARS: usize = 4096;
/// Longest error kept on a delivery row.
const MAX_ERROR_CHARS: usize = 500;

/// Telegram bot credentials (`BUZZ_CRM_TELEGRAM_*`).
#[derive(Clone)]
pub struct TelegramBot {
    token: String,
    /// Bot `@username` without the `@`, for invite links.
    pub username: Option<String>,
    /// Secret Telegram sends in `X-Telegram-Bot-Api-Secret-Token`.
    webhook_secret: Option<String>,
    api_base: String,
}

impl std::fmt::Debug for TelegramBot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramBot")
            .field("username", &self.username)
            .field("api_base", &self.api_base)
            .finish_non_exhaustive()
    }
}

/// Reminder worker configuration and shared HTTP client.
#[derive(Debug, Clone)]
pub struct ReminderRuntime {
    /// Whether this relay runs the worker.
    pub enabled: bool,
    /// Tick period.
    pub interval: Duration,
    /// The bot; `None` runs the sandbox transport.
    pub telegram: Option<TelegramBot>,
    http: reqwest::Client,
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

impl ReminderRuntime {
    /// Read `BUZZ_CRM_REMINDERS_ENABLED`, `BUZZ_CRM_REMINDER_INTERVAL_SECS`
    /// and `BUZZ_CRM_TELEGRAM_{BOT_TOKEN,BOT_USERNAME,WEBHOOK_SECRET,API_BASE}`.
    pub fn from_env() -> Self {
        let enabled = !matches!(
            env_nonempty("BUZZ_CRM_REMINDERS_ENABLED").as_deref(),
            Some("0" | "false" | "off" | "no")
        );
        let interval_secs = match env_nonempty("BUZZ_CRM_REMINDER_INTERVAL_SECS") {
            Some(raw) => raw.parse::<u64>().ok().filter(|s| *s > 0).unwrap_or_else(|| {
                warn!(%raw, "BUZZ_CRM_REMINDER_INTERVAL_SECS is not a positive integer; using 30");
                30
            }),
            None => 30,
        };
        let telegram = env_nonempty("BUZZ_CRM_TELEGRAM_BOT_TOKEN").map(|token| TelegramBot {
            token,
            username: env_nonempty("BUZZ_CRM_TELEGRAM_BOT_USERNAME")
                .map(|name| name.trim_start_matches('@').to_owned()),
            webhook_secret: env_nonempty("BUZZ_CRM_TELEGRAM_WEBHOOK_SECRET"),
            api_base: env_nonempty("BUZZ_CRM_TELEGRAM_API_BASE")
                .unwrap_or_else(|| "https://api.telegram.org".into())
                .trim_end_matches('/')
                .to_owned(),
        });
        let http = reqwest::Client::builder()
            .timeout(SEND_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            enabled,
            interval: Duration::from_secs(interval_secs),
            telegram,
            http,
        }
    }

    /// `telegram` or `sandbox`.
    pub fn transport_name(&self) -> &'static str {
        if self.telegram.is_some() {
            "telegram"
        } else {
            "sandbox"
        }
    }

    /// The `/start` code that links `customer` of `community` to whichever
    /// chat opens it. Bound to the community, so a code never links a
    /// customer of another salon on the same relay.
    pub fn start_code(&self, community: CommunityId, customer: Uuid) -> String {
        let mut raw = customer.as_bytes().to_vec();
        raw.extend_from_slice(&self.code_mac(community, customer)[..12]);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
    }

    /// The customer a `/start` code links, if the code is genuine.
    pub fn verify_start_code(&self, community: CommunityId, code: &str) -> Option<Uuid> {
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(code.trim())
            .ok()?;
        if raw.len() != 28 {
            return None;
        }
        let customer = Uuid::from_slice(&raw[..16]).ok()?;
        let expected = self.code_mac(community, customer);
        // Constant-time comparison of the truncated tag.
        use subtle::ConstantTimeEq as _;
        bool::from(expected[..12].ct_eq(&raw[16..])).then_some(customer)
    }

    fn code_mac(&self, community: CommunityId, customer: Uuid) -> Vec<u8> {
        let key = self
            .telegram
            .as_ref()
            .map_or("sandbox", |bot| bot.token.as_str());
        // A block-sized key makes the HMAC constructor infallible; HMAC pads
        // shorter keys with zeros, so this is HMAC keyed with the digest.
        let mut block = [0_u8; 64];
        block[..32].copy_from_slice(&Sha256::digest(format!("buzz-crm-telegram:{key}")));
        let mut mac = <Hmac<Sha256> as KeyInit>::new(&block.into());
        mac.update(community.as_uuid().as_bytes());
        mac.update(customer.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }

    /// `https://t.me/<bot>?start=<code>` when a bot with a username is set.
    pub fn invite_url(&self, community: CommunityId, customer: Uuid) -> Option<String> {
        let username = self.telegram.as_ref()?.username.as_deref()?;
        Some(format!(
            "https://t.me/{username}?start={}",
            self.start_code(community, customer)
        ))
    }

    /// Whether `header` is the configured webhook secret. A bot without a
    /// secret accepts no webhook calls.
    pub fn webhook_authorized(&self, header: Option<&str>) -> bool {
        use subtle::ConstantTimeEq as _;
        match (
            self.telegram
                .as_ref()
                .and_then(|bot| bot.webhook_secret.as_deref()),
            header,
        ) {
            (Some(secret), Some(given)) => bool::from(secret.as_bytes().ct_eq(given.as_bytes())),
            _ => false,
        }
    }

    /// Send one message through the configured transport.
    pub async fn send(&self, chat_id: i64, text: &str) -> SendOutcome {
        let Some(bot) = &self.telegram else {
            return SendOutcome::Sent { message_id: None };
        };
        let url = format!("{}/bot{}/sendMessage", bot.api_base, bot.token);
        let body = json!({"chat_id": chat_id, "text": text, "disable_web_page_preview": true});
        let response = match self.http.post(url).json(&body).send().await {
            Ok(response) => response,
            // `without_url`: the URL carries the bot token.
            Err(e) => {
                return SendOutcome::Retry {
                    error: e.without_url().to_string(),
                    after: None,
                }
            }
        };
        let status = response.status().as_u16();
        let reply: TelegramReply = response.json().await.unwrap_or_default();
        classify(status, reply)
    }
}

#[derive(Debug, Default, Deserialize)]
struct TelegramReply {
    #[serde(default)]
    ok: bool,
    result: Option<TelegramMessage>,
    description: Option<String>,
    parameters: Option<TelegramParameters>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    message_id: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramParameters {
    retry_after: Option<i64>,
}

/// What became of one send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    /// Delivered (the sandbox has no message id).
    Sent {
        /// Telegram message id.
        message_id: Option<i64>,
    },
    /// The client blocked the bot or never started it.
    Blocked(String),
    /// Telegram refused the message for good (bad chat, bad text).
    Rejected(String),
    /// Worth another try.
    Retry {
        /// What went wrong.
        error: String,
        /// Telegram's `retry_after`, seconds.
        after: Option<i64>,
    },
}

fn classify(status: u16, reply: TelegramReply) -> SendOutcome {
    let description = || {
        reply
            .description
            .clone()
            .unwrap_or_else(|| format!("HTTP {status}"))
    };
    match status {
        200 if reply.ok => SendOutcome::Sent {
            message_id: reply.result.as_ref().map(|m| m.message_id),
        },
        403 => SendOutcome::Blocked(description()),
        400 | 404 => SendOutcome::Rejected(description()),
        _ => SendOutcome::Retry {
            error: description(),
            after: reply.parameters.as_ref().and_then(|p| p.retry_after),
        },
    }
}

/// Delay before attempt `attempts + 1`: 1, 2, 4, 8 minutes, capped at 15.
fn retry_delay_secs(attempts: i32) -> i64 {
    let exponent = attempts.clamp(1, 5) - 1;
    (60_i64 << exponent).min(900)
}

/// Fill a rule's template for a visit. Never empty, at most Telegram's limit.
pub fn render_candidate(candidate: &ReminderCandidate) -> String {
    let services = candidate
        .services
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|service| service["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let context = ReminderContext {
        client: candidate.client_name.clone(),
        date: format_visit_date(&candidate.day.format("%Y-%m-%d").to_string()),
        time: u16::try_from(candidate.start_min)
            .map(buzz_crm::format_clock)
            .unwrap_or_default(),
        services,
        staff: candidate.staff_name.clone(),
        salon: candidate.salon_name.clone().unwrap_or_default(),
        address: candidate.address.clone().unwrap_or_default(),
        total: format_rub(candidate.total_minor),
    };
    let text = render(&candidate.template, &context);
    let text = text.trim();
    if text.is_empty() {
        return "—".to_owned();
    }
    text.chars().take(MAX_MESSAGE_CHARS).collect()
}

/// Whether the candidate can be sent, or why it is skipped.
fn plan_status(candidate: &ReminderCandidate) -> (&'static str, Option<&'static str>) {
    match (candidate.chat_id, candidate.opted_out) {
        (_, true) => ("skipped", Some("opted_out")),
        (None, false) => ("skipped", Some("no_telegram")),
        (Some(_), false) => ("pending", None),
    }
}

/// What a claimed delivery should do now, judged against the live state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Send(i64),
    Drop(&'static str, &'static str),
}

fn verdict(claimed: &ClaimedDelivery, now: chrono::DateTime<Utc>) -> Verdict {
    if !claimed.enabled {
        return Verdict::Drop("cancelled", "disabled");
    }
    let live = matches!(
        claimed.visit_status.as_deref(),
        Some("booked" | "confirmed")
    );
    let moved =
        claimed.rule_key != "booked" && claimed.live_slot_key.as_deref() != Some(&claimed.slot_key);
    if !live || moved {
        return Verdict::Drop("cancelled", "visit_changed");
    }
    if claimed.visit_at <= now {
        return Verdict::Drop("skipped", "visit_passed");
    }
    match (claimed.chat_id, claimed.opted_out) {
        (_, true) => Verdict::Drop("skipped", "opted_out"),
        (None, false) => Verdict::Drop("skipped", "no_telegram"),
        (Some(chat), false) => Verdict::Send(chat),
    }
}

fn truncate_error(error: &str) -> String {
    error.chars().take(MAX_ERROR_CHARS).collect()
}

/// Run the worker until the process exits.
pub async fn run(state: Arc<AppState>) {
    let runtime = Arc::clone(&state.crm_reminders);
    info!(
        transport = runtime.transport_name(),
        interval_secs = runtime.interval.as_secs(),
        "CRM reminder worker started"
    );
    let mut interval = tokio::time::interval(runtime.interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        tick(&state, &runtime).await;
    }
}

/// Plan what came due, then send what is pending.
pub async fn tick(state: &AppState, runtime: &ReminderRuntime) {
    match state.db.crm_due_reminders(GRACE_SECS, PLAN_BATCH).await {
        Ok(candidates) => {
            for candidate in &candidates {
                let text = render_candidate(candidate);
                let (status, reason) = plan_status(candidate);
                let planned = PlannedDelivery {
                    candidate,
                    text: &text,
                    status,
                    reason,
                };
                match state.db.crm_plan_reminder_delivery(planned).await {
                    Ok(true) => {
                        metrics::counter!("buzz_crm_reminders_planned_total", "status" => status)
                            .increment(1)
                    }
                    Ok(false) => {}
                    Err(e) => warn!(
                        appointment = %candidate.appointment_id,
                        rule = %candidate.rule_key,
                        "CRM reminder planning failed: {e}"
                    ),
                }
            }
        }
        Err(e) => error!("CRM reminder planning query failed: {e}"),
    }

    let claimed = match state
        .db
        .crm_claim_reminder_deliveries(LEASE_SECS, SEND_BATCH)
        .await
    {
        Ok(claimed) => claimed,
        Err(e) => {
            error!("CRM reminder claim failed: {e}");
            return;
        }
    };
    let claimed_at = tokio::time::Instant::now();
    for delivery in &claimed {
        // Rows left when the lease is nearly over are left to the next claim,
        // so this pod never sends a row another pod may have re-claimed.
        if claimed_at.elapsed() >= LEASE_SAFE_SEND {
            warn!("CRM reminder batch outran its lease; the rest is retried");
            break;
        }
        deliver(state, runtime, delivery).await;
    }
}

async fn deliver(state: &AppState, runtime: &ReminderRuntime, delivery: &ClaimedDelivery) {
    let transport = Some(runtime.transport_name());
    let finish = |status, reason, chat_id, message_id, last_error, retry_at| DeliveryFinish {
        attempt: delivery.attempts,
        status,
        reason,
        transport,
        chat_id,
        provider_message_id: message_id,
        last_error,
        retry_at,
    };
    let chat = match verdict(delivery, Utc::now()) {
        Verdict::Send(chat) => chat,
        Verdict::Drop(status, reason) => {
            record(
                state,
                delivery,
                DeliveryFinish {
                    transport: None,
                    ..finish(status, Some(reason), None, None, None, None)
                },
            )
            .await;
            return;
        }
    };
    let outcome = runtime.send(chat, &delivery.text).await;
    let error_text;
    let result = match &outcome {
        SendOutcome::Sent { message_id } => {
            finish("sent", None, Some(chat), *message_id, None, None)
        }
        SendOutcome::Blocked(error) => {
            error_text = truncate_error(error);
            let community = CommunityId::from_uuid(delivery.community_id);
            match state.db.crm_set_chat_opted_out(community, chat, true).await {
                Ok(_) => finish(
                    "failed",
                    Some("blocked"),
                    Some(chat),
                    None,
                    Some(&error_text),
                    None,
                ),
                // Keep the row pending: the next attempt meets the same 403
                // and records the opt-out again.
                Err(e) if delivery.attempts < MAX_ATTEMPTS => {
                    warn!("CRM reminder: marking a blocked chat failed: {e}");
                    finish(
                        "pending",
                        None,
                        Some(chat),
                        None,
                        Some(&error_text),
                        Some(Utc::now() + TimeDelta::seconds(retry_delay_secs(delivery.attempts))),
                    )
                }
                Err(e) => {
                    error!("CRM reminder: a blocked chat stays opted in: {e}");
                    finish(
                        "failed",
                        Some("blocked"),
                        Some(chat),
                        None,
                        Some(&error_text),
                        None,
                    )
                }
            }
        }
        SendOutcome::Rejected(error) => {
            error_text = truncate_error(error);
            finish(
                "failed",
                Some("error"),
                Some(chat),
                None,
                Some(&error_text),
                None,
            )
        }
        SendOutcome::Retry { error, after } => {
            error_text = truncate_error(error);
            if delivery.attempts >= MAX_ATTEMPTS {
                finish(
                    "failed",
                    Some("error"),
                    Some(chat),
                    None,
                    Some(&error_text),
                    None,
                )
            } else {
                let delay = after
                    .filter(|secs| *secs > 0)
                    .unwrap_or_else(|| retry_delay_secs(delivery.attempts))
                    .min(3600);
                let retry_at = Utc::now() + TimeDelta::seconds(delay);
                finish(
                    "pending",
                    None,
                    Some(chat),
                    None,
                    Some(&error_text),
                    Some(retry_at),
                )
            }
        }
    };
    record(state, delivery, result).await;
}

async fn record(state: &AppState, delivery: &ClaimedDelivery, finish: DeliveryFinish<'_>) {
    let status = finish.status.to_owned();
    match state
        .db
        .crm_finish_reminder_delivery(delivery.community_id, delivery.delivery_id, finish)
        .await
    {
        Ok(_) => metrics::counter!("buzz_crm_reminders_total", "status" => status).increment(1),
        // The row stays pending under its lease and is retried when the lease
        // ends, so a lost write never drops the message.
        Err(e) => warn!(delivery = %delivery.delivery_id, "CRM reminder outcome not recorded: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use serde_json::json;

    fn runtime(token: Option<&str>) -> ReminderRuntime {
        ReminderRuntime {
            enabled: true,
            interval: Duration::from_secs(30),
            telegram: token.map(|token| TelegramBot {
                token: token.into(),
                username: Some("salon_bot".into()),
                webhook_secret: Some("s3cret".into()),
                api_base: "http://127.0.0.1:1".into(),
            }),
            http: reqwest::Client::new(),
        }
    }

    fn candidate() -> ReminderCandidate {
        ReminderCandidate {
            community_id: Uuid::new_v4(),
            space_channel_id: Uuid::new_v4(),
            appointment_id: Uuid::new_v4(),
            rule_key: "before:1440".into(),
            slot_key: "2026-11-02 10:30".into(),
            template: "{client}, {date} в {time}: {services} у {staff}, {total}. {salon} {address}"
                .into(),
            salon_name: Some("Барбершоп".into()),
            address: None,
            day: NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
            start_min: 630,
            services: json!([{"name": "Стрижка"}, {"name": "Борода"}]),
            total_minor: 240_000,
            customer_id: None,
            client_name: "Анна".into(),
            staff_name: "Олег".into(),
            chat_id: Some(7),
            opted_out: false,
            due_at: Utc::now(),
            visit_at: Utc::now(),
        }
    }

    fn claimed() -> ClaimedDelivery {
        ClaimedDelivery {
            community_id: Uuid::new_v4(),
            delivery_id: Uuid::new_v4(),
            rule_key: "before:120".into(),
            slot_key: "2026-11-02 10:30".into(),
            text: "x".into(),
            attempts: 1,
            visit_at: Utc::now() + TimeDelta::hours(2),
            visit_status: Some("confirmed".into()),
            live_slot_key: Some("2026-11-02 10:30".into()),
            enabled: true,
            chat_id: Some(7),
            opted_out: false,
        }
    }

    #[test]
    fn candidates_render_with_visit_details() {
        assert_eq!(
            render_candidate(&candidate()),
            "Анна, пн, 2 ноября в 10:30: Стрижка, Борода у Олег, 2\u{a0}400\u{a0}₽. Барбершоп"
        );
        let mut blank = candidate();
        blank.template = "{address}".into();
        assert_eq!(render_candidate(&blank), "—");
        let mut long = candidate();
        long.template = "я".repeat(1000);
        long.services = json!((0..20)
            .map(|_| json!({"name": "x".repeat(128)}))
            .collect::<Vec<_>>());
        long.template.push_str(&"{services}".repeat(2));
        assert_eq!(render_candidate(&long).chars().count(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn clients_without_a_chat_are_skipped_at_planning() {
        let mut c = candidate();
        assert_eq!(plan_status(&c), ("pending", None));
        c.chat_id = None;
        assert_eq!(plan_status(&c), ("skipped", Some("no_telegram")));
        c.opted_out = true;
        assert_eq!(plan_status(&c), ("skipped", Some("opted_out")));
    }

    #[test]
    fn claimed_deliveries_follow_the_live_visit() {
        let now = Utc::now();
        assert_eq!(verdict(&claimed(), now), Verdict::Send(7));
        let mut moved = claimed();
        moved.live_slot_key = Some("2026-11-02 12:00".into());
        assert_eq!(
            verdict(&moved, now),
            Verdict::Drop("cancelled", "visit_changed")
        );
        // A booking confirmation does not depend on the visit time.
        moved.rule_key = "booked".into();
        assert_eq!(verdict(&moved, now), Verdict::Send(7));
        let mut cancelled = claimed();
        cancelled.visit_status = Some("cancelled".into());
        assert_eq!(
            verdict(&cancelled, now),
            Verdict::Drop("cancelled", "visit_changed")
        );
        let mut gone = claimed();
        gone.visit_status = None;
        gone.live_slot_key = None;
        assert_eq!(
            verdict(&gone, now),
            Verdict::Drop("cancelled", "visit_changed")
        );
        let mut off = claimed();
        off.enabled = false;
        assert_eq!(verdict(&off, now), Verdict::Drop("cancelled", "disabled"));
        let mut past = claimed();
        past.visit_at = now - TimeDelta::minutes(1);
        assert_eq!(
            verdict(&past, now),
            Verdict::Drop("skipped", "visit_passed")
        );
        let mut out = claimed();
        out.opted_out = true;
        assert_eq!(verdict(&out, now), Verdict::Drop("skipped", "opted_out"));
        let mut unlinked = claimed();
        unlinked.chat_id = None;
        assert_eq!(
            verdict(&unlinked, now),
            Verdict::Drop("skipped", "no_telegram")
        );
    }

    #[test]
    fn telegram_replies_are_classified() {
        let reply =
            |value: serde_json::Value| serde_json::from_value::<TelegramReply>(value).unwrap();
        assert_eq!(
            classify(200, reply(json!({"ok": true, "result": {"message_id": 5}}))),
            SendOutcome::Sent {
                message_id: Some(5)
            }
        );
        assert!(matches!(
            classify(403, reply(json!({"ok": false, "description": "Forbidden: bot was blocked by the user"}))),
            SendOutcome::Blocked(m) if m.contains("blocked")
        ));
        assert!(matches!(
            classify(
                400,
                reply(json!({"ok": false, "description": "Bad Request: chat not found"}))
            ),
            SendOutcome::Rejected(_)
        ));
        assert_eq!(
            classify(
                429,
                reply(
                    json!({"ok": false, "description": "Too Many Requests", "parameters": {"retry_after": 7}})
                )
            ),
            SendOutcome::Retry {
                error: "Too Many Requests".into(),
                after: Some(7)
            }
        );
        assert!(matches!(
            classify(502, TelegramReply::default()),
            SendOutcome::Retry { after: None, .. }
        ));
        assert_eq!(
            [1, 2, 3, 4, 5, 9].map(retry_delay_secs),
            [60, 120, 240, 480, 900, 900]
        );
    }

    #[test]
    fn start_codes_are_bound_to_community_and_bot() {
        let bot = runtime(Some("123:abc"));
        let community = CommunityId::from_uuid(Uuid::new_v4());
        let customer = Uuid::new_v4();
        let code = bot.start_code(community, customer);
        assert!(code.len() <= 64);
        assert!(code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_eq!(bot.verify_start_code(community, &code), Some(customer));
        let other = CommunityId::from_uuid(Uuid::new_v4());
        assert_eq!(bot.verify_start_code(other, &code), None);
        assert_eq!(
            runtime(Some("456:def")).verify_start_code(community, &code),
            None
        );
        let mut forged = code.clone();
        forged.replace_range(30..31, if &code[30..31] == "A" { "B" } else { "A" });
        assert_eq!(bot.verify_start_code(community, &forged), None);
        assert_eq!(bot.verify_start_code(community, "short"), None);
        assert_eq!(
            bot.invite_url(community, customer),
            Some(format!("https://t.me/salon_bot?start={code}"))
        );
        assert_eq!(runtime(None).invite_url(community, customer), None);
    }

    #[test]
    fn the_webhook_needs_the_secret() {
        let bot = runtime(Some("123:abc"));
        assert!(bot.webhook_authorized(Some("s3cret")));
        assert!(!bot.webhook_authorized(Some("guess")));
        assert!(!bot.webhook_authorized(None));
        assert!(!runtime(None).webhook_authorized(Some("s3cret")));
        // The token never shows up in debug output.
        assert!(!format!("{bot:?}").contains("123:abc"));
    }

    #[tokio::test]
    async fn the_sandbox_sends_without_a_network() {
        assert_eq!(
            runtime(None).send(7, "hi").await,
            SendOutcome::Sent { message_id: None }
        );
        // An unreachable API is a retry, and the error never carries the token.
        match runtime(Some("123:abc")).send(7, "hi").await {
            SendOutcome::Retry { error, .. } => assert!(!error.contains("123:abc"), "{error}"),
            other => panic!("{other:?}"),
        }
    }
}
