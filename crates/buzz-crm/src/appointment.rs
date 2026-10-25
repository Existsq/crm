//! Appointments (kind 42133): a client booked with one staff member for one
//! or more services on a calendar day.
//!
//! The payload carries the services with their booked duration and price, so
//! a visit can run longer than the catalog default or be discounted without
//! touching the catalog. The relay checks the booking against the staff
//! member's working hours and their other appointments inside the write
//! transaction; [`free_starts`] is the same arithmetic for the slot picker.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::catalog::{DURATION_STEP_MIN, MAX_DURATION_MIN};
use crate::error::{CrmError, Result};
use crate::money::Money;
use crate::party::phone;
use crate::space::parse_versioned;
use crate::staff::{check_date, format_clock, parse_clock, WorkInterval, DAY_MINUTES};
use crate::validate;

/// Maximum services on one appointment.
pub const MAX_APPOINTMENT_SERVICES: usize = 20;
/// Maximum appointment note length in characters.
pub const MAX_APPOINTMENT_NOTE_CHARS: usize = 500;

/// Where an appointment is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppointmentStatus {
    /// Booked, not confirmed by the client yet.
    Booked,
    /// The client confirmed they will come.
    Confirmed,
    /// The client is in the salon.
    Arrived,
    /// The visit happened (and is usually paid).
    Done,
    /// The client did not come.
    NoShow,
    /// Cancelled by the client or the salon.
    Cancelled,
}

impl AppointmentStatus {
    /// Every value, in display order.
    pub const ALL: [AppointmentStatus; 6] = [
        AppointmentStatus::Booked,
        AppointmentStatus::Confirmed,
        AppointmentStatus::Arrived,
        AppointmentStatus::Done,
        AppointmentStatus::NoShow,
        AppointmentStatus::Cancelled,
    ];

    /// Canonical wire name (`"no_show"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            AppointmentStatus::Booked => "booked",
            AppointmentStatus::Confirmed => "confirmed",
            AppointmentStatus::Arrived => "arrived",
            AppointmentStatus::Done => "done",
            AppointmentStatus::NoShow => "no_show",
            AppointmentStatus::Cancelled => "cancelled",
        }
    }

    /// Whether the appointment holds its time: cancelled and missed visits
    /// free the slot for someone else.
    pub const fn holds_time(self) -> bool {
        !matches!(
            self,
            AppointmentStatus::NoShow | AppointmentStatus::Cancelled
        )
    }
}

/// How the appointment was made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppointmentSource {
    /// Entered by the salon (phone call, walk-in).
    #[default]
    Staff,
    /// Requested by the client on the online booking page.
    Online,
}

impl AppointmentSource {
    /// Canonical wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            AppointmentSource::Staff => "staff",
            AppointmentSource::Online => "online",
        }
    }
}

/// One booked service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppointmentService {
    /// Bookable catalog product.
    pub product_id: Uuid,
    /// Booked duration in minutes (5–720, a multiple of 5).
    pub duration_min: u32,
    /// Price agreed for this service, kopecks.
    pub price_minor: Money,
}

/// Content of a kind 42133 event (the appointment id is the `crm-id` tag).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppointmentPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Staff member the client is booked with.
    pub staff_id: Uuid,
    /// Calendar date `YYYY-MM-DD` in the space timezone.
    pub date: String,
    /// Start, minutes after midnight (wire: `HH:MM`).
    #[serde(with = "clock")]
    pub start: u16,
    /// Services in the order they are performed.
    pub services: Vec<AppointmentService>,
    /// Client from the customer base, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_id: Option<Uuid>,
    /// Client name for a booking without a customer card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    /// Client phone for a booking without a customer card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_phone: Option<String>,
    /// Where the appointment is in its life.
    pub status: AppointmentStatus,
    /// Free-form note for the staff member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// How the appointment was made.
    #[serde(default)]
    pub source: AppointmentSource,
    /// Book outside the staff member's working hours (never over another
    /// appointment). Only salon staff may set it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outside_hours: bool,
}

mod clock {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(minutes: &u16, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::format_clock(*minutes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
        let raw = String::deserialize(deserializer)?;
        super::parse_clock(&raw).map_err(serde::de::Error::custom)
    }
}

impl AppointmentPayload {
    /// Parse, normalize and validate kind 42133 content.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: AppointmentPayload = parse_versioned(content)?;
        check_date(&payload.date)?;
        if payload.services.is_empty() || payload.services.len() > MAX_APPOINTMENT_SERVICES {
            return Err(CrmError::field(
                "services",
                format!("must list 1–{MAX_APPOINTMENT_SERVICES} services"),
            ));
        }
        let mut seen = HashSet::new();
        for service in &payload.services {
            if !seen.insert(service.product_id) {
                return Err(CrmError::field(
                    "services",
                    format!("{} is listed twice", service.product_id),
                ));
            }
            let duration = service.duration_min;
            if duration == 0
                || duration > MAX_DURATION_MIN
                || !duration.is_multiple_of(DURATION_STEP_MIN)
            {
                return Err(CrmError::field(
                    "duration_min",
                    format!(
                        "must be {DURATION_STEP_MIN}–{MAX_DURATION_MIN} minutes in steps of {DURATION_STEP_MIN}"
                    ),
                ));
            }
            if service.price_minor.minor() < 0 {
                return Err(CrmError::field("price_minor", "must not be negative"));
            }
        }
        if !payload.start.is_multiple_of(DURATION_STEP_MIN as u16) {
            return Err(CrmError::field(
                "start",
                format!("must be on a {DURATION_STEP_MIN}-minute grid"),
            ));
        }
        if u32::from(payload.start) + payload.duration_min() > u32::from(DAY_MINUTES) {
            return Err(CrmError::field(
                "services",
                "the visit must end by midnight",
            ));
        }
        payload.client_name = trimmed(payload.client_name.as_deref())
            .map(|name| validate::name(name, "client_name"))
            .transpose()?;
        payload.client_phone = trimmed(payload.client_phone.as_deref())
            .map(phone)
            .transpose()?;
        if payload.customer_id.is_none() && payload.client_name.is_none() {
            return Err(CrmError::field(
                "customer_id",
                "name the client: a customer_id or a client_name",
            ));
        }
        payload.note = trimmed(payload.note.as_deref())
            .map(|note| {
                if note.chars().count() > MAX_APPOINTMENT_NOTE_CHARS {
                    Err(CrmError::field(
                        "note",
                        format!("must be at most {MAX_APPOINTMENT_NOTE_CHARS} characters"),
                    ))
                } else {
                    Ok(note.to_owned())
                }
            })
            .transpose()?;
        Ok(payload)
    }

    /// Total booked minutes.
    pub fn duration_min(&self) -> u32 {
        self.services.iter().map(|s| s.duration_min).sum()
    }

    /// End, minutes after midnight (exclusive).
    pub fn end(&self) -> u16 {
        // `parse` keeps start + duration within one day.
        (u32::from(self.start) + self.duration_min()) as u16
    }

    /// Sum of the booked service prices, kopecks.
    pub fn total_minor(&self) -> i64 {
        self.services.iter().map(|s| s.price_minor.minor()).sum()
    }
}

/// Whether `start..end` lies inside one of the working `intervals`.
pub fn within_hours(intervals: &[WorkInterval], start: u16, end: u16) -> bool {
    intervals.iter().any(|i| i.start <= start && end <= i.end)
}

/// Whether `start..end` overlaps any of the `busy` ranges.
pub fn overlaps(busy: &[(u16, u16)], start: u16, end: u16) -> bool {
    busy.iter().any(|&(s, e)| start < e && s < end)
}

/// Start times (minutes after midnight) for a visit of `duration` minutes on
/// a `step`-minute grid inside the working `intervals`, clear of the `busy`
/// ranges and not before `not_before`.
pub fn free_starts(
    intervals: &[WorkInterval],
    busy: &[(u16, u16)],
    duration: u16,
    step: u16,
    not_before: u16,
) -> Vec<u16> {
    let mut starts = Vec::new();
    if duration == 0 || step == 0 {
        return starts;
    }
    for interval in intervals {
        // Align the first candidate to the grid from midnight.
        let first = interval.start.max(not_before).div_ceil(step) * step;
        let mut start = first;
        while u32::from(start) + u32::from(duration) <= u32::from(interval.end) {
            let end = start + duration;
            if !overlaps(busy, start, end) {
                starts.push(start);
            }
            start += step;
        }
    }
    starts
}

fn trimmed(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const STAFF: &str = "7d1b6c8e-1a2b-4c3d-8e9f-0a1b2c3d4e5f";
    const HAIRCUT: &str = "8e2c7d9f-2b3c-4d4e-9fa0-1b2c3d4e5f60";
    const BEARD: &str = "9f3d8ea0-3c4d-4e5f-a0b1-2c3d4e5f6071";

    fn booking() -> serde_json::Value {
        json!({
            "v": 1,
            "staff_id": STAFF,
            "date": "2026-11-02",
            "start": "10:30",
            "services": [
                {"product_id": HAIRCUT, "duration_min": 45, "price_minor": 150_000},
                {"product_id": BEARD, "duration_min": 30, "price_minor": 90_000}
            ],
            "client_name": " Пётр ",
            "client_phone": " +7 900 000-00-01 ",
            "status": "booked",
            "note": "  "
        })
    }

    fn parse(value: serde_json::Value) -> Result<AppointmentPayload> {
        AppointmentPayload::parse(&value.to_string())
    }

    #[test]
    fn a_booking_is_normalized_and_measured() {
        let payload = parse(booking()).unwrap();
        assert_eq!(payload.start, 630);
        assert_eq!(payload.end(), 705);
        assert_eq!(payload.duration_min(), 75);
        assert_eq!(payload.total_minor(), 240_000);
        assert_eq!(payload.client_name.as_deref(), Some("Пётр"));
        assert_eq!(payload.client_phone.as_deref(), Some("+7 900 000-00-01"));
        assert_eq!(payload.note, None);
        assert_eq!(payload.source, AppointmentSource::Staff);
        let wire = serde_json::to_value(&payload).unwrap();
        assert_eq!(wire["start"], json!("10:30"));
        assert!(wire.get("outside_hours").is_none());
        assert_eq!(parse(wire).unwrap(), payload);
    }

    #[test]
    fn invalid_bookings_are_rejected() {
        let cases: Vec<(&str, serde_json::Value)> = vec![
            ("start", json!("10:32")),
            ("start", json!("23:30")),
            ("date", json!("2026-02-30")),
            ("services", json!([])),
            (
                "services",
                json!([
                    {"product_id": HAIRCUT, "duration_min": 45, "price_minor": 1},
                    {"product_id": HAIRCUT, "duration_min": 45, "price_minor": 1}
                ]),
            ),
            (
                "services",
                json!([{"product_id": HAIRCUT, "duration_min": 42, "price_minor": 1}]),
            ),
            ("client_phone", json!("call me")),
        ];
        for (field, value) in cases {
            let mut raw = booking();
            raw[field] = value.clone();
            assert!(parse(raw).is_err(), "{field} = {value}");
        }
        let mut anonymous = booking();
        anonymous["client_name"] = json!(" ");
        assert!(parse(anonymous).is_err());
        let mut known = booking();
        known["client_name"] = json!(null);
        known["customer_id"] = json!(HAIRCUT);
        assert!(parse(known).is_ok());
    }

    #[test]
    fn cancelled_and_missed_visits_free_their_time() {
        let holding: Vec<_> = AppointmentStatus::ALL
            .iter()
            .filter(|s| s.holds_time())
            .map(|s| s.as_str())
            .collect();
        assert_eq!(holding, ["booked", "confirmed", "arrived", "done"]);
    }

    #[test]
    fn free_starts_skip_busy_time_and_breaks() {
        let day = [
            WorkInterval {
                start: 600,
                end: 840,
            },
            WorkInterval {
                start: 900,
                end: 1020,
            },
        ];
        // 10:00–14:00 and 15:00–17:00, busy 11:00–12:00, 45-minute visit.
        let starts = free_starts(&day, &[(660, 720)], 45, 15, 0);
        let shown: Vec<_> = starts.iter().map(|&m| format_clock(m)).collect();
        assert_eq!(
            shown,
            [
                "10:00", "10:15", "12:00", "12:15", "12:30", "12:45", "13:00", "13:15", "15:00",
                "15:15", "15:30", "15:45", "16:00", "16:15"
            ]
        );
        // Not before 12:40: the grid resumes at 12:45.
        assert_eq!(free_starts(&day, &[], 45, 15, 760)[0], 765);
        assert!(free_starts(&day, &[], 300, 15, 0).is_empty());
        assert!(within_hours(&day, 900, 1020));
        assert!(!within_hours(&day, 800, 905));
        assert!(overlaps(&[(660, 720)], 700, 760));
        assert!(!overlaps(&[(660, 720)], 720, 760));
    }
}
