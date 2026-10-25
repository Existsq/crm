//! Public online booking: the salon's page settings (kind 42135) and the
//! rules a client's booking must meet before the relay signs it as a kind
//! 42134 event.
//!
//! A booking made on the page is an [`AppointmentPayload`] with source
//! `online` and status `booked`; the relay prices it from the catalog, so the
//! client chooses only the staff member, the services, the day and the start.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::appointment::MAX_APPOINTMENT_SERVICES;
use crate::catalog::DURATION_STEP_MIN;
use crate::error::{CrmError, Result};
use crate::party::{phone, phone_digits};
use crate::space::parse_versioned;
use crate::validate;

/// Maximum `about` length in characters.
pub const MAX_ABOUT_CHARS: usize = 1000;
/// Maximum `address` length in characters.
pub const MAX_ADDRESS_CHARS: usize = 256;
/// Longest booking horizon, days.
pub const MAX_HORIZON_DAYS: u32 = 180;
/// Longest minimum notice, minutes (one week).
pub const MAX_NOTICE_MIN: u32 = 7 * 24 * 60;
/// Most staff or services the page may list explicitly.
pub const MAX_PAGE_ITEMS: usize = 200;

/// Content of a kind 42135 event: the space's public booking page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnlineBookingSettings {
    /// Schema version, always 1.
    pub v: u64,
    /// Whether the page accepts bookings.
    pub enabled: bool,
    /// Page heading, usually the salon name.
    pub title: String,
    /// A few words about the salon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    /// Where the salon is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// Salon phone for questions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// How many days ahead clients may book (1–180, today is day 0).
    pub horizon_days: u32,
    /// Earliest booking, minutes from now.
    pub min_notice_min: u32,
    /// Grid of offered start times, minutes (5–60, a multiple of 5).
    pub step_min: u32,
    /// Staff offered on the page; empty offers every active staff member.
    #[serde(default)]
    pub staff_ids: Vec<Uuid>,
    /// Services offered on the page; empty offers every active service with
    /// a duration.
    #[serde(default)]
    pub service_ids: Vec<Uuid>,
}

impl OnlineBookingSettings {
    /// Parse, normalize and validate kind 42135 content.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: OnlineBookingSettings = parse_versioned(content)?;
        payload.title = validate::name(&payload.title, "title")?;
        payload.about = text(payload.about.as_deref(), "about", MAX_ABOUT_CHARS, true)?;
        payload.address = text(
            payload.address.as_deref(),
            "address",
            MAX_ADDRESS_CHARS,
            false,
        )?;
        payload.phone = trimmed(payload.phone.as_deref()).map(phone).transpose()?;
        if !(1..=MAX_HORIZON_DAYS).contains(&payload.horizon_days) {
            return Err(CrmError::field(
                "horizon_days",
                format!("must be 1–{MAX_HORIZON_DAYS} days"),
            ));
        }
        if payload.min_notice_min > MAX_NOTICE_MIN {
            return Err(CrmError::field(
                "min_notice_min",
                format!("must be at most {MAX_NOTICE_MIN} minutes"),
            ));
        }
        if !(DURATION_STEP_MIN..=60).contains(&payload.step_min)
            || !payload.step_min.is_multiple_of(DURATION_STEP_MIN)
        {
            return Err(CrmError::field(
                "step_min",
                format!("must be {DURATION_STEP_MIN}–60 minutes in steps of {DURATION_STEP_MIN}"),
            ));
        }
        for (field, ids) in [
            ("staff_ids", &mut payload.staff_ids),
            ("service_ids", &mut payload.service_ids),
        ] {
            let mut seen = HashSet::new();
            ids.retain(|id| seen.insert(*id));
            if ids.len() > MAX_PAGE_ITEMS {
                return Err(CrmError::field(
                    field,
                    format!("at most {MAX_PAGE_ITEMS} entries"),
                ));
            }
        }
        Ok(payload)
    }

    /// Whether the page offers this staff member.
    pub fn offers_staff(&self, staff_id: Uuid) -> bool {
        self.staff_ids.is_empty() || self.staff_ids.contains(&staff_id)
    }

    /// Whether the page offers this service.
    pub fn offers_service(&self, product_id: Uuid) -> bool {
        self.service_ids.is_empty() || self.service_ids.contains(&product_id)
    }
}

/// A booking as the client sends it from the page.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnlineBookingRequest {
    /// Phone verification the client passed.
    pub verification_id: Uuid,
    /// The code from the SMS.
    pub code: String,
    /// Staff member chosen.
    pub staff_id: Uuid,
    /// Calendar date `YYYY-MM-DD` in the space timezone.
    pub date: String,
    /// Start `HH:MM`.
    pub start: String,
    /// Services in the order they are performed.
    pub service_ids: Vec<Uuid>,
    /// How the client introduces themselves.
    pub client_name: String,
    /// A wish for the staff member.
    #[serde(default)]
    pub note: Option<String>,
}

impl OnlineBookingRequest {
    /// Shape checks that need no database: 1–20 distinct services and a
    /// six-digit code. The appointment rules run on the built payload.
    pub fn check(&self) -> Result<()> {
        let distinct: HashSet<_> = self.service_ids.iter().collect();
        if self.service_ids.is_empty()
            || self.service_ids.len() > MAX_APPOINTMENT_SERVICES
            || distinct.len() != self.service_ids.len()
        {
            return Err(CrmError::field(
                "service_ids",
                format!("must list 1–{MAX_APPOINTMENT_SERVICES} distinct services"),
            ));
        }
        if !is_code(&self.code) {
            return Err(CrmError::field("code", "must be the 6 digits from the SMS"));
        }
        Ok(())
    }
}

/// Number of digits in a phone verification code.
pub const CODE_DIGITS: usize = 6;

/// Whether `code` looks like a verification code.
pub fn is_code(code: &str) -> bool {
    code.len() == CODE_DIGITS && code.bytes().all(|b| b.is_ascii_digit())
}

/// Validate a client's phone and return `(display, key)`: the phone as typed
/// (trimmed) and its digits with a Russian trunk `8` turned into `7`, so
/// `8 900 …` and `+7 900 …` count as one number for limits and matching.
pub fn client_phone(raw: &str) -> Result<(String, String)> {
    let display = phone(raw.trim())?;
    Ok((display.clone(), phone_key(&display)))
}

/// Digits of a phone with the Russian trunk prefix normalized.
pub fn phone_key(raw: &str) -> String {
    let digits = phone_digits(raw);
    match digits.strip_prefix('8') {
        Some(rest) if digits.len() == 11 => format!("7{rest}"),
        _ => digits,
    }
}

fn trimmed(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|value| !value.is_empty())
}

fn text(
    raw: Option<&str>,
    field: &'static str,
    max: usize,
    multiline: bool,
) -> Result<Option<String>> {
    trimmed(raw)
        .map(|value| {
            let bad_char = value
                .chars()
                .any(|c| c.is_control() && !(multiline && c == '\n'));
            if value.chars().count() > max || bad_char {
                Err(CrmError::field(
                    field,
                    format!("must be at most {max} printable characters"),
                ))
            } else {
                Ok(value.to_owned())
            }
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings() -> serde_json::Value {
        json!({
            "v": 1,
            "enabled": true,
            "title": " Борода ",
            "about": "Стрижки\nи бритьё",
            "address": "  ",
            "horizon_days": 30,
            "min_notice_min": 60,
            "step_min": 15,
            "staff_ids": ["7d1b6c8e-1a2b-4c3d-8e9f-0a1b2c3d4e5f", "7d1b6c8e-1a2b-4c3d-8e9f-0a1b2c3d4e5f"]
        })
    }

    fn parse(value: serde_json::Value) -> Result<OnlineBookingSettings> {
        OnlineBookingSettings::parse(&value.to_string())
    }

    #[test]
    fn settings_are_normalized() {
        let parsed = parse(settings()).unwrap();
        assert_eq!(parsed.title, "Борода");
        assert_eq!(parsed.about.as_deref(), Some("Стрижки\nи бритьё"));
        assert_eq!(parsed.address, None);
        assert_eq!(parsed.staff_ids.len(), 1);
        assert!(parsed.service_ids.is_empty());
        assert!(parsed.offers_service(Uuid::from_u128(1)));
        assert!(!parsed.offers_staff(Uuid::from_u128(1)));
        let wire = serde_json::to_value(&parsed).unwrap();
        assert_eq!(parse(wire).unwrap(), parsed);
    }

    #[test]
    fn invalid_settings_are_rejected() {
        let cases = [
            ("title", json!(" ")),
            ("horizon_days", json!(0)),
            ("horizon_days", json!(181)),
            ("min_notice_min", json!(MAX_NOTICE_MIN + 1)),
            ("step_min", json!(7)),
            ("step_min", json!(90)),
            ("phone", json!("call me")),
            ("address", json!("line\nbreak")),
            ("extra", json!(true)),
        ];
        for (field, value) in cases {
            let mut raw = settings();
            raw[field] = value.clone();
            assert!(parse(raw).is_err(), "{field} = {value}");
        }
    }

    #[test]
    fn phones_share_a_key_across_trunk_prefixes() {
        assert_eq!(
            client_phone(" +7 (900) 000-00-01 ").unwrap().1,
            "79000000001"
        );
        assert_eq!(phone_key("8 900 000 00 01"), "79000000001");
        assert_eq!(phone_key("+44 20 7946 0958"), "442079460958");
        assert!(client_phone("call me").is_err());
    }

    #[test]
    fn requests_need_distinct_services_and_a_code() {
        let request = |ids: Vec<u128>, code: &str| OnlineBookingRequest {
            verification_id: Uuid::from_u128(1),
            code: code.into(),
            staff_id: Uuid::from_u128(2),
            date: "2026-11-02".into(),
            start: "10:00".into(),
            service_ids: ids.into_iter().map(Uuid::from_u128).collect(),
            client_name: "Анна".into(),
            note: None,
        };
        assert!(request(vec![3], "123456").check().is_ok());
        assert!(request(vec![], "123456").check().is_err());
        assert!(request(vec![3, 3], "123456").check().is_err());
        assert!(request(vec![3], "12345").check().is_err());
        assert!(request(vec![3], "12345a").check().is_err());
    }
}
