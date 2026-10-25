//! Visit reminders: the space's reminder rules (kind 42150) and a client's
//! Telegram subscription (kind 42151).
//!
//! The relay owns delivery. It plans one message per rule and visit, renders
//! the rule's template with [`render`] and sends it through the configured
//! Telegram bot (or a sandbox that only records it). Templates name their
//! substitutions with `{placeholder}`; an unknown placeholder is rejected when
//! the rules are saved, so a typo never reaches a client.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{CrmError, Result};
use crate::space::parse_versioned;
use crate::validate;

/// Most rules one space may keep.
pub const MAX_REMINDER_RULES: usize = 4;
/// Longest template, in characters (a Telegram message holds 4096).
pub const MAX_TEMPLATE_CHARS: usize = 1000;
/// Earliest "before the visit" offset, minutes.
pub const MIN_OFFSET_MIN: u32 = 30;
/// Latest "before the visit" offset, minutes (7 days).
pub const MAX_OFFSET_MIN: u32 = 7 * 24 * 60;
/// Offsets are on this grid, minutes.
pub const OFFSET_STEP_MIN: u32 = 15;
/// Longest salon address, in characters.
pub const MAX_ADDRESS_CHARS: usize = 200;

/// Substitutions a template may use, in the order the UI offers them.
pub const PLACEHOLDERS: [&str; 8] = [
    "client", "date", "time", "services", "staff", "salon", "address", "total",
];

/// When a rule fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReminderTrigger {
    /// Right after the visit is booked (a booking confirmation).
    Booked,
    /// `offset_min` minutes before the visit starts.
    Before,
}

/// One reminder rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReminderRule {
    /// When the rule fires.
    pub trigger: ReminderTrigger,
    /// Minutes before the visit; required for [`ReminderTrigger::Before`],
    /// absent for [`ReminderTrigger::Booked`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_min: Option<u32>,
    /// Message text with `{placeholder}` substitutions.
    pub template: String,
}

impl ReminderRule {
    /// Stable identity of the rule within the space: `booked` or
    /// `before:<minutes>`. Deliveries are keyed by it, so editing a rule's
    /// text never re-sends a message already sent for that rule.
    pub fn key(&self) -> String {
        match (self.trigger, self.offset_min) {
            (ReminderTrigger::Before, Some(offset)) => format!("before:{offset}"),
            _ => "booked".to_owned(),
        }
    }
}

/// Content of a kind 42150 event: the space's reminder rules, replaced as a
/// whole on every update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReminderSettingsPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Whether the relay sends anything at all.
    pub enabled: bool,
    /// Up to [`MAX_REMINDER_RULES`] rules with distinct keys.
    pub rules: Vec<ReminderRule>,
    /// Salon name for `{salon}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub salon_name: Option<String>,
    /// Salon address for `{address}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

impl ReminderSettingsPayload {
    /// Parse, normalize and validate kind 42150 content.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: ReminderSettingsPayload = parse_versioned(content)?;
        if payload.rules.len() > MAX_REMINDER_RULES {
            return Err(CrmError::field(
                "rules",
                format!("must list at most {MAX_REMINDER_RULES} rules"),
            ));
        }
        let mut keys = HashSet::new();
        for rule in &mut payload.rules {
            match (rule.trigger, rule.offset_min) {
                (ReminderTrigger::Booked, None) => {}
                (ReminderTrigger::Booked, Some(_)) => {
                    return Err(CrmError::field(
                        "offset_min",
                        "a booking confirmation has no offset",
                    ))
                }
                (ReminderTrigger::Before, None) => {
                    return Err(CrmError::field(
                        "offset_min",
                        "is required for a reminder before the visit",
                    ))
                }
                (ReminderTrigger::Before, Some(offset)) => {
                    if !(MIN_OFFSET_MIN..=MAX_OFFSET_MIN).contains(&offset)
                        || !offset.is_multiple_of(OFFSET_STEP_MIN)
                    {
                        return Err(CrmError::field(
                            "offset_min",
                            format!(
                                "must be {MIN_OFFSET_MIN}–{MAX_OFFSET_MIN} minutes in steps of {OFFSET_STEP_MIN}"
                            ),
                        ));
                    }
                }
            }
            if !keys.insert(rule.key()) {
                return Err(CrmError::field(
                    "rules",
                    format!("{} is listed twice", rule.key()),
                ));
            }
            rule.template = check_template(&rule.template)?;
        }
        payload.salon_name = trimmed(payload.salon_name.as_deref())
            .map(|name| validate::name(name, "salon_name"))
            .transpose()?;
        payload.address = trimmed(payload.address.as_deref())
            .map(|address| {
                if address.chars().count() > MAX_ADDRESS_CHARS {
                    Err(CrmError::field(
                        "address",
                        format!("must be at most {MAX_ADDRESS_CHARS} characters"),
                    ))
                } else {
                    Ok(address.to_owned())
                }
            })
            .transpose()?;
        Ok(payload)
    }
}

/// Content of a kind 42151 event: whether and where a customer gets
/// reminders in Telegram.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientTelegramPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Customer card.
    pub customer_id: Uuid,
    /// Telegram chat to write to; `None` unlinks the customer. A client links
    /// their own chat by opening the bot's invite link; staff set it by hand
    /// only for testing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<i64>,
    /// The client asked not to receive reminders.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub opted_out: bool,
}

impl ClientTelegramPayload {
    /// Parse and validate kind 42151 content.
    pub fn parse(content: &str) -> Result<Self> {
        let payload: ClientTelegramPayload = parse_versioned(content)?;
        if payload.chat_id == Some(0) {
            return Err(CrmError::field("chat_id", "is not a Telegram chat id"));
        }
        Ok(payload)
    }
}

/// Values substituted into a template.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReminderContext {
    /// Client name.
    pub client: String,
    /// Visit date, already formatted ([`format_visit_date`]).
    pub date: String,
    /// Visit start, `HH:MM`.
    pub time: String,
    /// Service names, comma-separated.
    pub services: String,
    /// Staff member name.
    pub staff: String,
    /// Salon name.
    pub salon: String,
    /// Salon address.
    pub address: String,
    /// Visit total, formatted (`1 500 ₽`).
    pub total: String,
}

impl ReminderContext {
    fn value(&self, placeholder: &str) -> Option<&str> {
        Some(match placeholder {
            "client" => &self.client,
            "date" => &self.date,
            "time" => &self.time,
            "services" => &self.services,
            "staff" => &self.staff,
            "salon" => &self.salon,
            "address" => &self.address,
            "total" => &self.total,
            _ => return None,
        })
    }
}

/// Substitute every `{placeholder}` of a validated template. Text in braces
/// that is not a placeholder is kept as is.
pub fn render(template: &str, context: &ReminderContext) -> String {
    let mut out = String::with_capacity(template.len() + 64);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after
            .find('}')
            .and_then(|close| Some((close, context.value(&after[..close])?)))
        {
            Some((close, value)) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Trim a template and check its length and placeholders.
fn check_template(raw: &str) -> Result<String> {
    let template = raw.trim();
    if template.is_empty() {
        return Err(CrmError::field("template", "must not be empty"));
    }
    if template.chars().count() > MAX_TEMPLATE_CHARS {
        return Err(CrmError::field(
            "template",
            format!("must be at most {MAX_TEMPLATE_CHARS} characters"),
        ));
    }
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            break;
        };
        let name = &after[..close];
        let looks_like_placeholder =
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if looks_like_placeholder && !PLACEHOLDERS.contains(&name) {
            return Err(CrmError::field(
                "template",
                format!(
                    "{{{name}}} is not a placeholder; use one of {}",
                    PLACEHOLDERS.map(|p| format!("{{{p}}}")).join(", ")
                ),
            ));
        }
        rest = &after[close + 1..];
    }
    Ok(template.to_owned())
}

const MONTHS_GENITIVE: [&str; 12] = [
    "января",
    "февраля",
    "марта",
    "апреля",
    "мая",
    "июня",
    "июля",
    "августа",
    "сентября",
    "октября",
    "ноября",
    "декабря",
];
const WEEKDAYS_SHORT: [&str; 7] = ["вс", "пн", "вт", "ср", "чт", "пт", "сб"];

/// A `YYYY-MM-DD` date as a client reads it: `пн, 2 ноября`. Returns the
/// input unchanged if it is not a valid date.
pub fn format_visit_date(date: &str) -> String {
    if crate::staff::check_date(date).is_err() {
        return date.to_owned();
    }
    let part = |range: std::ops::Range<usize>| date[range].parse::<u32>().unwrap_or(0);
    let (year, month, day) = (part(0..4), part(5..7), part(8..10));
    // Sakamoto's day of the week, 0 = Sunday.
    const OFFSETS: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    let weekday = (y + y / 4 - y / 100 + y / 400 + OFFSETS[month as usize - 1] + day) % 7;
    format!(
        "{}, {day} {}",
        WEEKDAYS_SHORT[weekday as usize],
        MONTHS_GENITIVE[month as usize - 1]
    )
}

/// Kopecks as roubles for a client message: `1 500 ₽`, `99,50 ₽`.
pub fn format_rub(minor: i64) -> String {
    let sign = if minor < 0 { "-" } else { "" };
    let minor = minor.unsigned_abs();
    let digits = (minor / 100).to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push('\u{a0}');
        }
        grouped.push(digit);
    }
    match minor % 100 {
        0 => format!("{sign}{grouped}\u{a0}₽"),
        kopecks => format!("{sign}{grouped},{kopecks:02}\u{a0}₽"),
    }
}

fn trimmed(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings() -> serde_json::Value {
        json!({
            "v": 1,
            "enabled": true,
            "rules": [
                {"trigger": "booked", "template": " {client}, вы записаны на {date} в {time}. "},
                {"trigger": "before", "offset_min": 1440, "template": "Завтра в {time} ждём вас: {services}"},
                {"trigger": "before", "offset_min": 120, "template": "Через 2 часа, мастер {staff}"}
            ],
            "salon_name": " Барбершоп ",
            "address": "  "
        })
    }

    fn parse(value: serde_json::Value) -> Result<ReminderSettingsPayload> {
        ReminderSettingsPayload::parse(&value.to_string())
    }

    #[test]
    fn rules_are_normalized_and_keyed() {
        let payload = parse(settings()).unwrap();
        let keys: Vec<_> = payload.rules.iter().map(ReminderRule::key).collect();
        assert_eq!(keys, ["booked", "before:1440", "before:120"]);
        assert_eq!(
            payload.rules[0].template,
            "{client}, вы записаны на {date} в {time}."
        );
        assert_eq!(payload.salon_name.as_deref(), Some("Барбершоп"));
        assert_eq!(payload.address, None);
        let wire = serde_json::to_value(&payload).unwrap();
        assert!(wire["rules"][0].get("offset_min").is_none());
        assert_eq!(parse(wire).unwrap(), payload);
    }

    #[test]
    fn invalid_rules_are_rejected() {
        let rule = |value: serde_json::Value| {
            let mut raw = settings();
            raw["rules"] = json!([value]);
            parse(raw)
        };
        for bad in [
            json!({"trigger": "before", "template": "x"}),
            json!({"trigger": "booked", "offset_min": 60, "template": "x"}),
            json!({"trigger": "before", "offset_min": 20, "template": "x"}),
            json!({"trigger": "before", "offset_min": 70, "template": "x"}),
            json!({"trigger": "before", "offset_min": 20160, "template": "x"}),
            json!({"trigger": "booked", "template": "  "}),
            json!({"trigger": "booked", "template": "Привет, {name}"}),
            json!({"trigger": "booked", "template": "x".repeat(1001)}),
            json!({"trigger": "later", "template": "x"}),
        ] {
            assert!(rule(bad.clone()).is_err(), "{bad}");
        }
        let mut twice = settings();
        twice["rules"][2]["offset_min"] = json!(1440);
        assert!(parse(twice).is_err());
        let mut many = settings();
        many["rules"] = json!((1..=5)
            .map(|h| json!({"trigger": "before", "offset_min": h * 60, "template": "x"}))
            .collect::<Vec<_>>());
        assert!(parse(many).is_err());
        // Braces that are not placeholder-shaped stay literal text.
        assert!(rule(json!({"trigger": "booked", "template": "{ скидка 10% } {}"})).is_ok());
    }

    #[test]
    fn templates_render_every_placeholder() {
        let context = ReminderContext {
            client: "Анна".into(),
            date: format_visit_date("2026-11-02"),
            time: "10:30".into(),
            services: "Стрижка, Борода".into(),
            staff: "Олег".into(),
            salon: "Барбершоп".into(),
            address: "ул. Ленина, 1".into(),
            total: format_rub(240_000),
        };
        let all = PLACEHOLDERS.map(|p| format!("{{{p}}}")).join("|");
        assert_eq!(
            render(&all, &context),
            "Анна|пн, 2 ноября|10:30|Стрижка, Борода|Олег|Барбершоп|ул. Ленина, 1|2\u{a0}400\u{a0}₽"
        );
        assert_eq!(render("{x} {client} {", &context), "{x} Анна {");
    }

    #[test]
    fn dates_and_money_read_naturally() {
        assert_eq!(format_visit_date("2026-10-25"), "вс, 25 октября");
        assert_eq!(format_visit_date("2024-02-29"), "чт, 29 февраля");
        assert_eq!(format_visit_date("2026-01-01"), "чт, 1 января");
        assert_eq!(format_visit_date("2026-02-30"), "2026-02-30");
        assert_eq!(format_rub(99_950), "999,50\u{a0}₽");
        assert_eq!(format_rub(123_456_700), "1\u{a0}234\u{a0}567\u{a0}₽");
        assert_eq!(format_rub(0), "0\u{a0}₽");
    }

    #[test]
    fn telegram_links_parse() {
        let id = Uuid::new_v4();
        let link = ClientTelegramPayload::parse(
            &json!({"v": 1, "customer_id": id, "chat_id": 123456789}).to_string(),
        )
        .unwrap();
        assert_eq!(link.chat_id, Some(123_456_789));
        assert!(!link.opted_out);
        assert!(ClientTelegramPayload::parse(
            &json!({"v": 1, "customer_id": id, "chat_id": 0}).to_string()
        )
        .is_err());
        let unlinked =
            ClientTelegramPayload::parse(&json!({"v": 1, "customer_id": id}).to_string()).unwrap();
        assert_eq!(unlinked.chat_id, None);
    }
}
