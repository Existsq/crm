//! `GET /crm/reminders`, `GET /crm/reminders/deliveries`,
//! `GET /crm/parties/{id}/telegram` — reminder rules, the delivery log and a
//! customer's Telegram link — and `POST /crm/telegram/webhook`, where the
//! salon's Telegram bot reports `/start`, `/stop` and `/resume`.

use std::sync::Arc;

use axum::{
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use tracing::warn;
use uuid::Uuid;

use buzz_crm::reminder::PLACEHOLDERS;
use buzz_db::crm::{BotLinkOutcome, DeliveryQuery, MAX_DELIVERY_PAGE};

use super::authorize_space_member;
use crate::{
    api::{api_error, internal_error, parse_query_or_400},
    state::AppState,
};

/// Route path of the reminder rules.
pub const REMINDERS_PATH: &str = "/crm/reminders";
/// Route path of the delivery log.
pub const DELIVERIES_PATH: &str = "/crm/reminders/deliveries";
/// Route path of a customer's Telegram link.
pub const PARTY_TELEGRAM_PATH: &str = "/crm/parties/{party_id}/telegram";
/// Route path of the Telegram bot webhook.
pub const TELEGRAM_WEBHOOK_PATH: &str = "/crm/telegram/webhook";

const DEFAULT_DELIVERY_LIMIT: i64 = 200;

/// `GET /crm/reminders` — the rules, the transport and the placeholders.
pub async fn settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member = match authorize_space_member(
        &state,
        &headers,
        REMINDERS_PATH,
        raw_query.as_deref(),
    )
    .await
    {
        Ok(member) => member,
        Err(response) => return response,
    };
    let record = match state
        .db
        .crm_get_reminder_settings(member.community(), member.space.channel_id)
        .await
    {
        Ok(record) => record,
        Err(e) => {
            return internal_error(&format!("CRM reminder settings read: {e}")).into_response()
        }
    };
    let runtime = &state.crm_reminders;
    Json(json!({
        "settings": record.as_ref().map(|r| &r.settings),
        "enabled": record.as_ref().is_some_and(|r| r.enabled),
        "revision": record.as_ref().map(|r| hex::encode(&r.revision_event_id)),
        "updated_at": record.as_ref().map(|r| r.updated_at),
        "transport": {
            "mode": runtime.transport_name(),
            "bot_username": runtime.telegram.as_ref().and_then(|bot| bot.username.clone()),
            "worker": runtime.enabled,
        },
        "placeholders": PLACEHOLDERS,
    }))
    .into_response()
}

/// Query parameters of `GET /crm/reminders/deliveries`.
#[derive(Debug, Deserialize, Default)]
pub struct DeliveryParams {
    since: Option<DateTime<Utc>>,
    appointment_id: Option<Uuid>,
    customer_id: Option<Uuid>,
    limit: Option<i64>,
}

fn delivery_query(params: DeliveryParams) -> Result<DeliveryQuery, String> {
    let limit = params.limit.unwrap_or(DEFAULT_DELIVERY_LIMIT);
    if !(1..=MAX_DELIVERY_PAGE).contains(&limit) {
        return Err(format!("limit must be 1–{MAX_DELIVERY_PAGE}"));
    }
    Ok(DeliveryQuery {
        since: params.since,
        appointment_id: params.appointment_id,
        customer_id: params.customer_id,
        limit,
    })
}

/// `GET /crm/reminders/deliveries` — the delivery log, newest first.
pub async fn deliveries(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let member =
        match authorize_space_member(&state, &headers, DELIVERIES_PATH, raw_query.as_deref()).await
        {
            Ok(member) => member,
            Err(response) => return response,
        };
    let params: DeliveryParams = match parse_query_or_400(raw_query.as_deref()) {
        Ok(params) => params,
        Err(response) => return response.into_response(),
    };
    let query = match delivery_query(params) {
        Ok(query) => query,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, &message).into_response(),
    };
    match state
        .db
        .crm_list_reminder_deliveries(member.community(), member.space.channel_id, &query)
        .await
    {
        Ok(rows) => Json(json!({ "deliveries": rows })).into_response(),
        Err(e) => internal_error(&format!("CRM reminder deliveries read: {e}")).into_response(),
    }
}

/// `GET /crm/parties/{id}/telegram` — where a customer gets reminders and
/// the link that subscribes them.
pub async fn party_telegram(
    State(state): State<Arc<AppState>>,
    Path(party_id): Path<Uuid>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let path = format!("/crm/parties/{party_id}/telegram");
    let member = match authorize_space_member(&state, &headers, &path, raw_query.as_deref()).await {
        Ok(member) => member,
        Err(response) => return response,
    };
    let community = member.community();
    let link = match state
        .db
        .crm_get_client_telegram(community, member.space.channel_id, party_id)
        .await
    {
        Ok(link) => link,
        Err(e) => return internal_error(&format!("CRM Telegram link read: {e}")).into_response(),
    };
    let runtime = &state.crm_reminders;
    Json(json!({
        "customer_id": party_id,
        "linked": link.as_ref().is_some_and(|l| l.chat_id.is_some()),
        "chat_id": link.as_ref().and_then(|l| l.chat_id),
        "username": link.as_ref().and_then(|l| l.username.clone()),
        "opted_out": link.as_ref().is_some_and(|l| l.opted_out),
        "linked_via": link.as_ref().map(|l| l.linked_via.clone()),
        "linked_at": link.as_ref().and_then(|l| l.linked_at),
        "invite_url": runtime.invite_url(community, party_id),
        "start_code": runtime.start_code(community, party_id),
    }))
    .into_response()
}

/// The parts of a Telegram `Update` the bot reacts to.
#[derive(Debug, Deserialize)]
pub struct TelegramUpdate {
    message: Option<TelegramIncoming>,
}

#[derive(Debug, Deserialize)]
struct TelegramIncoming {
    chat: TelegramChat,
    from: Option<TelegramUser>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    username: Option<String>,
}

/// A bot command the webhook understands.
#[derive(Debug, PartialEq, Eq)]
enum BotCommand<'a> {
    Start(Option<&'a str>),
    Stop,
    Resume,
}

fn bot_command(text: &str) -> Option<BotCommand<'_>> {
    let mut parts = text.split_whitespace();
    // `/start@salon_bot` in group chats names the bot after the command.
    let command = parts.next()?.split('@').next()?;
    let argument = parts.next();
    match command {
        "/start" => Some(BotCommand::Start(argument)),
        "/stop" => Some(BotCommand::Stop),
        "/resume" => Some(BotCommand::Resume),
        _ => None,
    }
}

/// `POST /crm/telegram/webhook` — Telegram's update push for the community
/// the host resolves to. Requires the configured secret header; answers 200
/// once an update is handled, and 500 when a link could not be stored so
/// Telegram redelivers it. Telegram allows one webhook URL per bot, so one
/// bot serves one community host.
pub async fn telegram_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let runtime = Arc::clone(&state.crm_reminders);
    let secret = headers
        .get("x-telegram-bot-api-secret-token")
        .and_then(|value| value.to_str().ok());
    if !runtime.webhook_authorized(secret) {
        return api_error(StatusCode::NOT_FOUND, "no Telegram bot here").into_response();
    }
    let raw_host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let Ok(tenant) = crate::tenant::bind_community(&state.db, raw_host).await else {
        return api_error(
            StatusCode::NOT_FOUND,
            "relay: no community is configured for this host",
        )
        .into_response();
    };
    let community = tenant.community();
    // Parsed only after the secret check, so strangers learn nothing from
    // validation errors. Updates the bot does not handle are acknowledged.
    let Some(message) = serde_json::from_slice::<TelegramUpdate>(&body)
        .ok()
        .and_then(|update| update.message)
    else {
        return Json(json!({"ok": true})).into_response();
    };
    let chat = message.chat.id;
    let Some(command) = message.text.as_deref().and_then(bot_command) else {
        return Json(json!({"ok": true})).into_response();
    };
    let reply = match command {
        BotCommand::Start(None) => {
            "Чтобы получать напоминания о визитах, откройте ссылку, которую прислал салон."
                .to_owned()
        }
        BotCommand::Start(Some(code)) => match runtime.verify_start_code(community, code) {
            None => "Ссылка недействительна. Попросите салон прислать новую.".to_owned(),
            Some(customer) => {
                let username = message.from.as_ref().and_then(|u| u.username.as_deref());
                match state
                    .db
                    .crm_link_client_telegram_from_bot(community, customer, chat, username)
                    .await
                {
                    Ok(BotLinkOutcome::Linked(name)) => format!(
                        "Готово, {name}! Мы пришлём сюда напоминания о ваших визитах. \
                         Чтобы отписаться, отправьте /stop."
                    ),
                    Ok(BotLinkOutcome::UnknownCustomer) => {
                        "Ссылка недействительна. Попросите салон прислать новую.".to_owned()
                    }
                    Ok(BotLinkOutcome::OtherChat) => {
                        "Эта ссылка уже подключена к другому чату. Если это ваша запись, \
                         попросите салон отвязать старый чат."
                            .to_owned()
                    }
                    Err(e) => {
                        // Telegram retries non-2xx updates; a failed link is retried.
                        return internal_error(&format!("CRM Telegram link: {e}")).into_response();
                    }
                }
            }
        },
        BotCommand::Stop | BotCommand::Resume => {
            let opted_out = command == BotCommand::Stop;
            if let Err(e) = state
                .db
                .crm_set_chat_opted_out(community, chat, opted_out)
                .await
            {
                return internal_error(&format!("CRM Telegram opt-out: {e}")).into_response();
            }
            if opted_out {
                "Вы отписались от напоминаний. Чтобы вернуться, отправьте /resume.".to_owned()
            } else {
                "Напоминания снова включены.".to_owned()
            }
        }
    };
    match runtime.send(chat, &reply).await {
        crate::crm_reminders::SendOutcome::Sent { .. } => {}
        other => warn!(?other, "CRM Telegram bot reply not delivered"),
    }
    Json(json!({"ok": true})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_commands_are_recognized() {
        assert_eq!(
            bot_command("/start abc_DEF-1"),
            Some(BotCommand::Start(Some("abc_DEF-1")))
        );
        assert_eq!(bot_command("/start"), Some(BotCommand::Start(None)));
        assert_eq!(
            bot_command("/start@salon_bot"),
            Some(BotCommand::Start(None))
        );
        assert_eq!(bot_command("  /stop "), Some(BotCommand::Stop));
        assert_eq!(bot_command("/resume"), Some(BotCommand::Resume));
        assert_eq!(bot_command("привет"), None);
        assert_eq!(bot_command(""), None);
    }

    #[test]
    fn delivery_limits_are_validated() {
        assert_eq!(
            delivery_query(DeliveryParams::default()).unwrap().limit,
            DEFAULT_DELIVERY_LIMIT
        );
        for bad in [0, MAX_DELIVERY_PAGE + 1] {
            let params = DeliveryParams {
                limit: Some(bad),
                ..DeliveryParams::default()
            };
            assert!(delivery_query(params).is_err(), "limit {bad}");
        }
    }
}
