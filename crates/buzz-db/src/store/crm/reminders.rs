//! CRM visit reminders (migration 0064, kinds 42150, 42151).
//!
//! Settings and staff-made Telegram links are projections written inside the
//! caller's event-write transaction. Links made by the bot and the delivery
//! log are relay-owned and written by the reminder worker on the pool: one
//! statement per row, so a fenced community never stalls the others.

use buzz_datastore_tracing::datastore_span;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row as _, Transaction};
use uuid::Uuid;

use super::hex_bytes;
use crate::{error::Result, CommunityId, Db};

/// Most deliveries one log read returns.
pub const MAX_DELIVERY_PAGE: i64 = 500;

/// A validated kind 42150 payload ready to project.
#[derive(Debug, Clone, Copy)]
pub struct ReminderSettingsWrite<'a> {
    /// CRM space channel.
    pub space_channel_id: Uuid,
    /// Normalized `ReminderSettingsPayload` as JSON.
    pub settings: &'a Value,
    /// Whether reminders are on.
    pub enabled: bool,
    /// Event id of this revision.
    pub revision_event_id: &'a [u8],
    /// Author pubkey.
    pub actor: &'a [u8],
}

/// Outcome of a settings write; anything but `Written` rolls back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderSettingsOutcome {
    /// Inserted or updated.
    Written,
    /// A first write found settings already saved.
    AlreadyExists,
    /// An update found no settings.
    Missing,
    /// The settings changed since `expected_revision`.
    RevisionMismatch,
}

/// Current reminder settings of a space.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReminderSettingsRecord {
    /// Normalized payload.
    pub settings: Value,
    /// Whether reminders are on.
    pub enabled: bool,
    /// Current revision event id.
    #[serde(rename = "revision", serialize_with = "hex_bytes")]
    pub revision_event_id: Vec<u8>,
    /// Last change.
    pub updated_at: DateTime<Utc>,
}

/// A validated kind 42151 payload ready to project.
#[derive(Debug, Clone, Copy)]
pub struct ClientTelegramWrite<'a> {
    /// CRM space channel.
    pub space_channel_id: Uuid,
    /// Customer card.
    pub customer_id: Uuid,
    /// Chat to write to; `None` unlinks.
    pub chat_id: Option<i64>,
    /// The client asked not to receive reminders.
    pub opted_out: bool,
    /// Event id of this revision.
    pub revision_event_id: &'a [u8],
}

/// Outcome of a link write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientTelegramOutcome {
    /// Inserted or updated.
    Written,
    /// The customer is not a customer of this space.
    UnknownCustomer,
}

/// Where a customer gets reminders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClientTelegramRecord {
    /// Customer card.
    pub customer_id: Uuid,
    /// Linked chat, if any.
    pub chat_id: Option<i64>,
    /// Telegram `@username` reported by the bot.
    pub username: Option<String>,
    /// The client asked not to receive reminders (or blocked the bot).
    pub opted_out: bool,
    /// `staff` or `bot`.
    pub linked_via: String,
    /// When the current chat was linked.
    pub linked_at: Option<DateTime<Utc>>,
}

/// One row of the delivery log.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReminderDeliveryRecord {
    /// Delivery id.
    #[serde(rename = "id")]
    pub delivery_id: Uuid,
    /// Visit.
    pub appointment_id: Uuid,
    /// `booked` or `before:<minutes>`.
    pub rule_key: String,
    /// Customer card, if any.
    pub customer_id: Option<Uuid>,
    /// Client name at planning time.
    pub client_name: String,
    /// Chat written to.
    pub chat_id: Option<i64>,
    /// Rendered message.
    pub text: String,
    /// `pending`, `sent`, `failed`, `skipped` or `cancelled`.
    pub status: String,
    /// Why it was not sent.
    pub reason: Option<String>,
    /// `telegram` or `sandbox`.
    pub transport: Option<String>,
    /// Send attempts so far.
    pub attempts: i32,
    /// Last transport error.
    pub last_error: Option<String>,
    /// When the message was due.
    pub due_at: DateTime<Utc>,
    /// Visit start.
    pub visit_at: DateTime<Utc>,
    /// When it was planned.
    pub created_at: DateTime<Utc>,
    /// When it was sent.
    pub sent_at: Option<DateTime<Utc>>,
}

/// Filters of a delivery log read.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeliveryQuery {
    /// Planned at or after.
    pub since: Option<DateTime<Utc>>,
    /// Only this visit.
    pub appointment_id: Option<Uuid>,
    /// Only this customer.
    pub customer_id: Option<Uuid>,
    /// Page size, capped at [`MAX_DELIVERY_PAGE`].
    pub limit: i64,
}

/// A rule that is due for a visit and has no delivery yet.
#[derive(Debug, Clone, PartialEq)]
pub struct ReminderCandidate {
    /// Community.
    pub community_id: Uuid,
    /// CRM space channel.
    pub space_channel_id: Uuid,
    /// Visit.
    pub appointment_id: Uuid,
    /// Rule identity (`booked`, `before:<minutes>`).
    pub rule_key: String,
    /// Visit time the rule is planned for (empty for `booked`).
    pub slot_key: String,
    /// Rule template.
    pub template: String,
    /// `salon_name` setting.
    pub salon_name: Option<String>,
    /// `address` setting.
    pub address: Option<String>,
    /// Visit day in the space timezone.
    pub day: NaiveDate,
    /// Visit start, minutes after midnight.
    pub start_min: i32,
    /// Service snapshot `[{"name", …}]`.
    pub services: Value,
    /// Visit total, kopecks.
    pub total_minor: i64,
    /// Customer card.
    pub customer_id: Option<Uuid>,
    /// Client name (card or booking).
    pub client_name: String,
    /// Staff member name.
    pub staff_name: String,
    /// Linked chat.
    pub chat_id: Option<i64>,
    /// The client opted out.
    pub opted_out: bool,
    /// When the rule was due.
    pub due_at: DateTime<Utc>,
    /// Visit start.
    pub visit_at: DateTime<Utc>,
}

/// A planned delivery row.
#[derive(Debug, Clone, Copy)]
pub struct PlannedDelivery<'a> {
    /// The candidate it was planned from.
    pub candidate: &'a ReminderCandidate,
    /// Rendered text.
    pub text: &'a str,
    /// `pending` or `skipped`.
    pub status: &'a str,
    /// Why it is skipped.
    pub reason: Option<&'a str>,
}

/// A pending delivery claimed for sending, with the live state it must be
/// checked against before it goes out.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedDelivery {
    /// Community.
    pub community_id: Uuid,
    /// Delivery id.
    pub delivery_id: Uuid,
    /// Rule identity.
    pub rule_key: String,
    /// Visit time the message was planned for.
    pub slot_key: String,
    /// Rendered text.
    pub text: String,
    /// Attempts including this one.
    pub attempts: i32,
    /// Visit start as planned.
    pub visit_at: DateTime<Utc>,
    /// Current visit status (`None` when the visit is gone).
    pub visit_status: Option<String>,
    /// Current visit time key, same format as `slot_key`.
    pub live_slot_key: Option<String>,
    /// Whether reminders are still on for the space.
    pub enabled: bool,
    /// Currently linked chat.
    pub chat_id: Option<i64>,
    /// The client opted out since planning.
    pub opted_out: bool,
}

/// Final or retry state of a claimed delivery.
#[derive(Debug, Clone, Copy)]
pub struct DeliveryFinish<'a> {
    /// The claim's `attempts`: the outcome is written only while the row is
    /// still on that claim, so a sender whose lease ran out never overwrites
    /// a newer claim.
    pub attempt: i32,
    /// `sent`, `failed`, `skipped`, `cancelled`, or `pending` to retry.
    pub status: &'a str,
    /// Why it was not sent.
    pub reason: Option<&'a str>,
    /// Transport used.
    pub transport: Option<&'a str>,
    /// Chat written to.
    pub chat_id: Option<i64>,
    /// Telegram message id.
    pub provider_message_id: Option<i64>,
    /// Transport error (truncated by the caller to 500 characters).
    pub last_error: Option<&'a str>,
    /// Next attempt, for `pending`.
    pub retry_at: Option<DateTime<Utc>>,
}

/// Visit time key of a row: `YYYY-MM-DD HH:MM`.
macro_rules! slot_key_sql {
    ($day:expr, $start:expr) => {
        concat!(
            "(to_char(",
            $day,
            ", 'YYYY-MM-DD') || ' ' || lpad((",
            $start,
            " / 60)::text, 2, '0') || ':' || lpad((",
            $start,
            " % 60)::text, 2, '0'))"
        )
    };
}

/// Save the first settings (no `expected_revision`) or CAS-update them.
pub async fn write_reminder_settings(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    write: ReminderSettingsWrite<'_>,
    expected_revision: Option<&[u8]>,
) -> Result<ReminderSettingsOutcome> {
    use ReminderSettingsOutcome as Out;
    let current: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT revision_event_id FROM crm_reminder_settings \
         WHERE community_id = $1 AND space_channel_id = $2 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(write.space_channel_id)
    .fetch_optional(&mut **tx)
    .await?;
    match (expected_revision, current) {
        (None, Some(_)) => return Ok(Out::AlreadyExists),
        (Some(_), None) => return Ok(Out::Missing),
        (Some(expected), Some(revision)) if revision != expected => {
            return Ok(Out::RevisionMismatch)
        }
        _ => {}
    }
    sqlx::query(
        "INSERT INTO crm_reminder_settings \
           (community_id, space_channel_id, settings, enabled, revision_event_id, updated_by) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (community_id, space_channel_id) DO UPDATE SET \
           settings = EXCLUDED.settings, enabled = EXCLUDED.enabled, \
           revision_event_id = EXCLUDED.revision_event_id, \
           updated_by = EXCLUDED.updated_by, updated_at = now()",
    )
    .bind(community.as_uuid())
    .bind(write.space_channel_id)
    .bind(write.settings)
    .bind(write.enabled)
    .bind(write.revision_event_id)
    .bind(write.actor)
    .execute(&mut **tx)
    .await?;
    Ok(Out::Written)
}

/// Link, unlink or opt out a customer by staff. Changing the chat resets the
/// username the bot reported.
pub async fn write_client_telegram(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    write: ClientTelegramWrite<'_>,
) -> Result<ClientTelegramOutcome> {
    let customer: Option<Uuid> = sqlx::query_scalar(
        "SELECT party_id FROM crm_parties \
         WHERE community_id = $1 AND space_channel_id = $2 AND party_id = $3 \
           AND party_type IN ('customer', 'both') FOR SHARE",
    )
    .bind(community.as_uuid())
    .bind(write.space_channel_id)
    .bind(write.customer_id)
    .fetch_optional(&mut **tx)
    .await?;
    if customer.is_none() {
        return Ok(ClientTelegramOutcome::UnknownCustomer);
    }
    sqlx::query(
        "INSERT INTO crm_client_telegram AS t \
           (community_id, customer_id, space_channel_id, chat_id, opted_out, linked_via, \
            revision_event_id, linked_at) \
         VALUES ($1, $2, $3, $4, $5, 'staff', $6, CASE WHEN $4::bigint IS NULL THEN NULL ELSE now() END) \
         ON CONFLICT (community_id, customer_id) DO UPDATE SET \
           chat_id = EXCLUDED.chat_id, opted_out = EXCLUDED.opted_out, \
           revision_event_id = EXCLUDED.revision_event_id, updated_at = now(), \
           linked_via = CASE WHEN t.chat_id IS NOT DISTINCT FROM EXCLUDED.chat_id \
                             THEN t.linked_via ELSE 'staff' END, \
           username = CASE WHEN t.chat_id IS NOT DISTINCT FROM EXCLUDED.chat_id \
                           THEN t.username END, \
           linked_at = CASE WHEN t.chat_id IS NOT DISTINCT FROM EXCLUDED.chat_id \
                            THEN t.linked_at ELSE EXCLUDED.linked_at END",
    )
    .bind(community.as_uuid())
    .bind(write.customer_id)
    .bind(write.space_channel_id)
    .bind(write.chat_id)
    .bind(write.opted_out)
    .bind(write.revision_event_id)
    .execute(&mut **tx)
    .await?;
    Ok(ClientTelegramOutcome::Written)
}

/// The space's reminder settings.
pub async fn get_reminder_settings(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
) -> Result<Option<ReminderSettingsRecord>> {
    sqlx::query(
        "SELECT settings, enabled, revision_event_id, updated_at FROM crm_reminder_settings \
         WHERE community_id = $1 AND space_channel_id = $2",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .fetch_optional(pool)
    .await?
    .map(|row| {
        Ok(ReminderSettingsRecord {
            settings: row.try_get("settings")?,
            enabled: row.try_get("enabled")?,
            revision_event_id: row.try_get("revision_event_id")?,
            updated_at: row.try_get("updated_at")?,
        })
    })
    .transpose()
}

/// Where a customer of the space gets reminders.
pub async fn get_client_telegram(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    customer_id: Uuid,
) -> Result<Option<ClientTelegramRecord>> {
    sqlx::query(
        "SELECT customer_id, chat_id, username, opted_out, linked_via, linked_at \
         FROM crm_client_telegram \
         WHERE community_id = $1 AND space_channel_id = $2 AND customer_id = $3",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(customer_id)
    .fetch_optional(pool)
    .await?
    .map(|row| {
        Ok(ClientTelegramRecord {
            customer_id: row.try_get("customer_id")?,
            chat_id: row.try_get("chat_id")?,
            username: row.try_get("username")?,
            opted_out: row.try_get("opted_out")?,
            linked_via: row.try_get("linked_via")?,
            linked_at: row.try_get("linked_at")?,
        })
    })
    .transpose()
}

/// Outcome of a `/start` from an invite link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BotLinkOutcome {
    /// The chat is linked; the customer's name.
    Linked(String),
    /// The customer is not in the community's space (a stale or forged link).
    UnknownCustomer,
    /// The customer is linked to another chat. A forwarded invite never takes
    /// over a client's reminders; staff unlink the old chat first.
    OtherChat,
}

/// Link a customer to the chat that opened their invite link, unless another
/// chat is already linked. Opening the link again from the same chat opts the
/// client back in.
pub async fn link_client_telegram_from_bot(
    pool: &PgPool,
    community: CommunityId,
    customer_id: Uuid,
    chat_id: i64,
    username: Option<&str>,
) -> Result<BotLinkOutcome> {
    let mut tx = pool.begin().await?;
    let customer = sqlx::query(
        "SELECT p.space_channel_id, p.name FROM crm_parties p \
         JOIN crm_spaces s ON s.community_id = p.community_id AND s.channel_id = p.space_channel_id \
         WHERE p.community_id = $1 AND p.party_id = $2 AND p.party_type IN ('customer', 'both') \
         FOR SHARE OF p",
    )
    .bind(community.as_uuid())
    .bind(customer_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(customer) = customer else {
        return Ok(BotLinkOutcome::UnknownCustomer);
    };
    let space: Uuid = customer.try_get("space_channel_id")?;
    let name: String = customer.try_get("name")?;
    let linked = sqlx::query(
        "INSERT INTO crm_client_telegram \
           (community_id, customer_id, space_channel_id, chat_id, username, opted_out, \
            linked_via, linked_at) \
         VALUES ($1, $2, $3, $4, $5, false, 'bot', now()) \
         ON CONFLICT (community_id, customer_id) DO UPDATE SET \
           chat_id = EXCLUDED.chat_id, username = EXCLUDED.username, opted_out = false, \
           linked_via = 'bot', revision_event_id = NULL, \
           linked_at = CASE WHEN crm_client_telegram.chat_id IS NULL THEN now() \
                            ELSE crm_client_telegram.linked_at END, \
           updated_at = now() \
         WHERE crm_client_telegram.chat_id IS NULL \
            OR crm_client_telegram.chat_id = EXCLUDED.chat_id",
    )
    .bind(community.as_uuid())
    .bind(customer_id)
    .bind(space)
    .bind(chat_id)
    .bind(username)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(if linked == 1 {
        BotLinkOutcome::Linked(name)
    } else {
        BotLinkOutcome::OtherChat
    })
}

/// Opt every customer linked to `chat_id` in or out (the client sent
/// `/stop`, blocked the bot, or came back). Returns the number of customers.
pub async fn set_chat_opted_out(
    pool: &PgPool,
    community: CommunityId,
    chat_id: i64,
    opted_out: bool,
) -> Result<u64> {
    Ok(sqlx::query(
        "UPDATE crm_client_telegram SET opted_out = $3, updated_at = now() \
         WHERE community_id = $1 AND chat_id = $2 AND opted_out <> $3",
    )
    .bind(community.as_uuid())
    .bind(chat_id)
    .bind(opted_out)
    .execute(pool)
    .await?
    .rows_affected())
}

/// The space's delivery log, newest first.
pub async fn list_deliveries(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    query: &DeliveryQuery,
) -> Result<Vec<ReminderDeliveryRecord>> {
    sqlx::query(
        "SELECT delivery_id, appointment_id, rule_key, customer_id, client_name, chat_id, text, \
                status, reason, transport, attempts, last_error, due_at, visit_at, created_at, \
                sent_at \
         FROM crm_reminder_deliveries \
         WHERE community_id = $1 AND space_channel_id = $2 \
           AND ($3::timestamptz IS NULL OR created_at >= $3) \
           AND ($4::uuid IS NULL OR appointment_id = $4) \
           AND ($5::uuid IS NULL OR customer_id = $5) \
         ORDER BY created_at DESC, delivery_id LIMIT $6",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(query.since)
    .bind(query.appointment_id)
    .bind(query.customer_id)
    .bind(query.limit.clamp(1, MAX_DELIVERY_PAGE))
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(ReminderDeliveryRecord {
            delivery_id: row.try_get("delivery_id")?,
            appointment_id: row.try_get("appointment_id")?,
            rule_key: row.try_get("rule_key")?,
            customer_id: row.try_get("customer_id")?,
            client_name: row.try_get("client_name")?,
            chat_id: row.try_get("chat_id")?,
            text: row.try_get("text")?,
            status: row.try_get("status")?,
            reason: row.try_get("reason")?,
            transport: row.try_get("transport")?,
            attempts: row.try_get("attempts")?,
            last_error: row.try_get("last_error")?,
            due_at: row.try_get("due_at")?,
            visit_at: row.try_get("visit_at")?,
            created_at: row.try_get("created_at")?,
            sent_at: row.try_get("sent_at")?,
        })
    })
    .collect()
}

/// Rules due now for upcoming visits that have no delivery yet, across every
/// active community, oldest due first.
///
/// A rule is due from its due time until `grace_secs` later; a relay that was
/// down longer skips it rather than reminding late. A "before" reminder only
/// fires for visits booked before it was due, so a visit booked an hour ahead
/// gets its booking confirmation, not also a "2 hours before" message.
pub async fn due_reminders(
    pool: &PgPool,
    grace_secs: i64,
    limit: i64,
) -> Result<Vec<ReminderCandidate>> {
    let sql = concat!(
        "SELECT r.community_id, r.space_channel_id, a.appointment_id, k.rule_key, k.slot_key, \
                rule->>'template' AS template, r.settings->>'salon_name' AS salon_name, \
                r.settings->>'address' AS address, a.day, a.start_min, a.services, \
                a.total_minor, a.customer_id, COALESCE(p.name, a.client_name, '—') AS client_name, \
                st.name AS staff_name, t.chat_id, COALESCE(t.opted_out, false) AS opted_out, \
                w.due_at, w.visit_at \
         FROM crm_reminder_settings r \
         JOIN communities c ON c.id = r.community_id AND c.deletion_state = 'active' \
         JOIN crm_spaces s ON s.community_id = r.community_id AND s.channel_id = r.space_channel_id \
         CROSS JOIN LATERAL jsonb_array_elements(r.settings->'rules') AS rule \
         CROSS JOIN LATERAL ( \
           SELECT (now() AT TIME ZONE (s.settings->>'timezone'))::date AS today) l \
         JOIN crm_appointments a \
           ON a.community_id = r.community_id AND a.space_channel_id = r.space_channel_id \
          AND a.day >= l.today - 1 AND a.status IN ('booked', 'confirmed') \
         CROSS JOIN LATERAL ( \
           SELECT ((a.day + make_interval(mins => a.start_min)) \
                     AT TIME ZONE (s.settings->>'timezone')) AS visit_at) v \
         CROSS JOIN LATERAL ( \
           SELECT v.visit_at, \
                  CASE WHEN rule->>'trigger' = 'booked' THEN a.created_at \
                       ELSE v.visit_at - make_interval(mins => (rule->>'offset_min')::int) \
                  END AS due_at) w \
         CROSS JOIN LATERAL ( \
           SELECT CASE WHEN rule->>'trigger' = 'booked' THEN 'booked' \
                       ELSE 'before:' || (rule->>'offset_min') END AS rule_key, \
                  CASE WHEN rule->>'trigger' = 'booked' THEN '' \
                       ELSE ",
        slot_key_sql!("a.day", "a.start_min"),
        " END AS slot_key) k \
         JOIN crm_staff st ON st.community_id = a.community_id AND st.staff_id = a.staff_id \
         LEFT JOIN crm_parties p ON p.community_id = a.community_id AND p.party_id = a.customer_id \
         LEFT JOIN crm_client_telegram t \
           ON t.community_id = a.community_id AND t.customer_id = a.customer_id \
         WHERE r.enabled \
           AND (CASE WHEN rule->>'trigger' = 'booked' \
                     THEN a.created_at > now() - make_interval(secs => $1) \
                     ELSE a.day <= l.today + 8 END) \
           AND w.due_at <= now() AND w.due_at > now() - make_interval(secs => $1) \
           AND w.visit_at > now() \
           AND (rule->>'trigger' = 'booked' OR a.created_at <= w.due_at) \
           AND NOT EXISTS ( \
             SELECT 1 FROM crm_reminder_deliveries d \
              WHERE d.community_id = a.community_id AND d.appointment_id = a.appointment_id \
                AND d.rule_key = k.rule_key AND d.slot_key = k.slot_key \
                AND d.status <> 'cancelled') \
         ORDER BY w.due_at LIMIT $2"
    );
    sqlx::query(sql)
        .bind(grace_secs as f64)
        .bind(limit)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| {
            Ok(ReminderCandidate {
                community_id: row.try_get("community_id")?,
                space_channel_id: row.try_get("space_channel_id")?,
                appointment_id: row.try_get("appointment_id")?,
                rule_key: row.try_get("rule_key")?,
                slot_key: row.try_get("slot_key")?,
                template: row.try_get("template")?,
                salon_name: row.try_get("salon_name")?,
                address: row.try_get("address")?,
                day: row.try_get("day")?,
                start_min: row.try_get("start_min")?,
                services: row.try_get("services")?,
                total_minor: row.try_get("total_minor")?,
                customer_id: row.try_get("customer_id")?,
                client_name: row.try_get("client_name")?,
                staff_name: row.try_get("staff_name")?,
                chat_id: row.try_get("chat_id")?,
                opted_out: row.try_get("opted_out")?,
                due_at: row.try_get("due_at")?,
                visit_at: row.try_get("visit_at")?,
            })
        })
        .collect()
}

/// Record a planned delivery. Returns `false` when another worker planned
/// the same (visit, rule, time) first. A delivery cancelled because the visit
/// was cancelled or reminders were off is planned afresh when the visit (or
/// the rule) is back.
pub async fn plan_delivery(pool: &PgPool, planned: PlannedDelivery<'_>) -> Result<bool> {
    let c = planned.candidate;
    let inserted = sqlx::query(
        "INSERT INTO crm_reminder_deliveries \
           (community_id, delivery_id, space_channel_id, appointment_id, rule_key, slot_key, \
            customer_id, client_name, chat_id, text, status, reason, due_at, visit_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
         ON CONFLICT (community_id, appointment_id, rule_key, slot_key) DO UPDATE SET \
           customer_id = EXCLUDED.customer_id, client_name = EXCLUDED.client_name, \
           chat_id = EXCLUDED.chat_id, text = EXCLUDED.text, status = EXCLUDED.status, \
           reason = EXCLUDED.reason, transport = NULL, provider_message_id = NULL, \
           attempts = 0, next_attempt_at = now(), last_error = NULL, \
           due_at = EXCLUDED.due_at, visit_at = EXCLUDED.visit_at, created_at = now(), \
           sent_at = NULL \
         WHERE crm_reminder_deliveries.status = 'cancelled'",
    )
    .bind(c.community_id)
    .bind(Uuid::new_v4())
    .bind(c.space_channel_id)
    .bind(c.appointment_id)
    .bind(&c.rule_key)
    .bind(&c.slot_key)
    .bind(c.customer_id)
    .bind(&c.client_name)
    .bind(c.chat_id)
    .bind(planned.text)
    .bind(planned.status)
    .bind(planned.reason)
    .bind(c.due_at)
    .bind(c.visit_at)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(inserted == 1)
}

/// Claim up to `limit` pending deliveries whose attempt is due: count the
/// attempt and hold them for `lease_secs`, so a crashed sender's rows come
/// back on their own. Returns them with the live visit and link state.
pub async fn claim_due_deliveries(
    pool: &PgPool,
    lease_secs: i64,
    limit: i64,
) -> Result<Vec<ClaimedDelivery>> {
    let sql = concat!(
        "WITH due AS ( \
           SELECT d.community_id, d.delivery_id FROM crm_reminder_deliveries d \
           JOIN communities c ON c.id = d.community_id AND c.deletion_state = 'active' \
           WHERE d.status = 'pending' AND d.next_attempt_at <= now() \
           ORDER BY d.next_attempt_at LIMIT $2 FOR UPDATE OF d SKIP LOCKED \
         ), claimed AS ( \
           UPDATE crm_reminder_deliveries d \
              SET attempts = d.attempts + 1, \
                  next_attempt_at = now() + make_interval(secs => $1) \
             FROM due WHERE d.community_id = due.community_id AND d.delivery_id = due.delivery_id \
           RETURNING d.community_id, d.delivery_id, d.space_channel_id, d.appointment_id, \
                     d.customer_id, d.rule_key, d.slot_key, d.text, d.attempts, d.visit_at \
         ) \
         SELECT cl.community_id, cl.delivery_id, cl.rule_key, cl.slot_key, cl.text, cl.attempts, \
                cl.visit_at, a.status AS visit_status, ",
        slot_key_sql!("a.day", "a.start_min"),
        " AS live_slot_key, COALESCE(r.enabled, false) AS enabled, t.chat_id, \
                COALESCE(t.opted_out, false) AS opted_out \
         FROM claimed cl \
         LEFT JOIN crm_appointments a \
           ON a.community_id = cl.community_id AND a.appointment_id = cl.appointment_id \
         LEFT JOIN crm_reminder_settings r \
           ON r.community_id = cl.community_id AND r.space_channel_id = cl.space_channel_id \
         LEFT JOIN crm_client_telegram t \
           ON t.community_id = cl.community_id AND t.customer_id = cl.customer_id"
    );
    sqlx::query(sql)
        .bind(lease_secs as f64)
        .bind(limit)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| {
            Ok(ClaimedDelivery {
                community_id: row.try_get("community_id")?,
                delivery_id: row.try_get("delivery_id")?,
                rule_key: row.try_get("rule_key")?,
                slot_key: row.try_get("slot_key")?,
                text: row.try_get("text")?,
                attempts: row.try_get("attempts")?,
                visit_at: row.try_get("visit_at")?,
                visit_status: row.try_get("visit_status")?,
                live_slot_key: row.try_get("live_slot_key")?,
                enabled: row.try_get("enabled")?,
                chat_id: row.try_get("chat_id")?,
                opted_out: row.try_get("opted_out")?,
            })
        })
        .collect()
}

/// Record the outcome of a claimed delivery. Only a row still pending on the
/// same claim changes, so a late writer never overwrites a final state or a
/// newer claim. Returns whether the row changed.
pub async fn finish_delivery(
    pool: &PgPool,
    community_id: Uuid,
    delivery_id: Uuid,
    finish: DeliveryFinish<'_>,
) -> Result<bool> {
    let updated = sqlx::query(
        "UPDATE crm_reminder_deliveries SET \
           status = $3, reason = $4, transport = COALESCE($5, transport), \
           chat_id = COALESCE($6, chat_id), provider_message_id = $7, last_error = $8, \
           sent_at = CASE WHEN $3 = 'sent' THEN now() END, \
           next_attempt_at = COALESCE($9, next_attempt_at) \
         WHERE community_id = $1 AND delivery_id = $2 AND status = 'pending' \
           AND attempts = $10",
    )
    .bind(community_id)
    .bind(delivery_id)
    .bind(finish.status)
    .bind(finish.reason)
    .bind(finish.transport)
    .bind(finish.chat_id)
    .bind(finish.provider_message_id)
    .bind(finish.last_error)
    .bind(finish.retry_at)
    .bind(finish.attempt)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(updated == 1)
}

impl Db {
    /// The space's reminder settings.
    #[datastore_span(name = "crm_get_reminder_settings", system = "postgresql")]
    pub async fn crm_get_reminder_settings(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
    ) -> Result<Option<ReminderSettingsRecord>> {
        get_reminder_settings(&self.pool, community, space_channel_id).await
    }

    /// Where a customer gets reminders.
    #[datastore_span(name = "crm_get_client_telegram", system = "postgresql")]
    pub async fn crm_get_client_telegram(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        customer_id: Uuid,
    ) -> Result<Option<ClientTelegramRecord>> {
        get_client_telegram(&self.pool, community, space_channel_id, customer_id).await
    }

    /// Link a customer from the bot's `/start`.
    #[datastore_span(name = "crm_link_client_telegram", system = "postgresql")]
    pub async fn crm_link_client_telegram_from_bot(
        &self,
        community: CommunityId,
        customer_id: Uuid,
        chat_id: i64,
        username: Option<&str>,
    ) -> Result<BotLinkOutcome> {
        link_client_telegram_from_bot(&self.pool, community, customer_id, chat_id, username).await
    }

    /// Opt a chat's customers in or out.
    #[datastore_span(name = "crm_set_chat_opted_out", system = "postgresql")]
    pub async fn crm_set_chat_opted_out(
        &self,
        community: CommunityId,
        chat_id: i64,
        opted_out: bool,
    ) -> Result<u64> {
        set_chat_opted_out(&self.pool, community, chat_id, opted_out).await
    }

    /// The space's delivery log.
    #[datastore_span(name = "crm_list_reminder_deliveries", system = "postgresql")]
    pub async fn crm_list_reminder_deliveries(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        query: &DeliveryQuery,
    ) -> Result<Vec<ReminderDeliveryRecord>> {
        list_deliveries(&self.pool, community, space_channel_id, query).await
    }

    /// Rules due now (reminder worker).
    #[datastore_span(name = "crm_due_reminders", system = "postgresql")]
    pub async fn crm_due_reminders(
        &self,
        grace_secs: i64,
        limit: i64,
    ) -> Result<Vec<ReminderCandidate>> {
        due_reminders(&self.pool, grace_secs, limit).await
    }

    /// Record a planned delivery (reminder worker).
    #[datastore_span(name = "crm_plan_reminder_delivery", system = "postgresql")]
    pub async fn crm_plan_reminder_delivery(&self, planned: PlannedDelivery<'_>) -> Result<bool> {
        plan_delivery(&self.pool, planned).await
    }

    /// Claim due deliveries (reminder worker).
    #[datastore_span(name = "crm_claim_reminder_deliveries", system = "postgresql")]
    pub async fn crm_claim_reminder_deliveries(
        &self,
        lease_secs: i64,
        limit: i64,
    ) -> Result<Vec<ClaimedDelivery>> {
        claim_due_deliveries(&self.pool, lease_secs, limit).await
    }

    /// Record a delivery outcome (reminder worker).
    #[datastore_span(name = "crm_finish_reminder_delivery", system = "postgresql")]
    pub async fn crm_finish_reminder_delivery(
        &self,
        community_id: Uuid,
        delivery_id: Uuid,
        finish: DeliveryFinish<'_>,
    ) -> Result<bool> {
        finish_delivery(&self.pool, community_id, delivery_id, finish).await
    }
}

#[cfg(test)]
mod postgres_tests;
