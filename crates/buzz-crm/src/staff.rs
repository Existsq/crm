//! Staff (masters) and their working schedule (kinds 42131, 42132).
//!
//! A staff member is who clients book: a name, the services they perform and,
//! optionally, the channel member who logs in as them. The schedule is kept
//! per calendar day, so a booking grid never has to expand recurrence rules.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{CrmError, Result};
use crate::party::phone;
use crate::space::parse_versioned;
use crate::validate;

/// Maximum position (job title) length in characters.
pub const MAX_POSITION_CHARS: usize = 64;
/// Maximum services one staff member is linked to.
pub const MAX_STAFF_SERVICES: usize = 200;
/// Maximum days one schedule event may set.
pub const MAX_SCHEDULE_DAYS: usize = 93;
/// Maximum working intervals per day (shift with breaks).
pub const MAX_DAY_INTERVALS: usize = 4;
/// Schedule grid step in minutes.
pub const SCHEDULE_STEP_MIN: u16 = 5;
/// Minutes in a day; an interval may end exactly at midnight.
pub const DAY_MINUTES: u16 = 24 * 60;

/// Content of a kind 42131 event (the staff id is the `crm-id` tag).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaffPayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Name clients see when booking (1–128 characters).
    pub name: String,
    /// Job title, e.g. «Барбер».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    /// Work phone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// Services (bookable products) this person performs.
    #[serde(default)]
    pub service_ids: Vec<Uuid>,
    /// Hex pubkey of the channel member who works as this staff member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    /// Archived staff take no new bookings and leave the journal.
    #[serde(default)]
    pub archived: bool,
}

impl StaffPayload {
    /// Parse, normalize and validate kind 42131 content.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: StaffPayload = parse_versioned(content)?;
        payload.name = validate::name(&payload.name, "name")?;
        payload.position = trimmed(payload.position.as_deref())
            .map(|value| {
                if value.chars().count() > MAX_POSITION_CHARS || value.chars().any(char::is_control)
                {
                    Err(CrmError::field(
                        "position",
                        format!("must be at most {MAX_POSITION_CHARS} printable characters"),
                    ))
                } else {
                    Ok(value.to_owned())
                }
            })
            .transpose()?;
        payload.phone = trimmed(payload.phone.as_deref()).map(phone).transpose()?;
        let mut seen = HashSet::new();
        payload.service_ids.retain(|id| seen.insert(*id));
        if payload.service_ids.len() > MAX_STAFF_SERVICES {
            return Err(CrmError::field(
                "service_ids",
                format!("at most {MAX_STAFF_SERVICES} services per staff member"),
            ));
        }
        payload.member = trimmed(payload.member.as_deref())
            .map(|hex| {
                let valid = hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit());
                if valid {
                    Ok(hex.to_ascii_lowercase())
                } else {
                    Err(CrmError::field(
                        "member",
                        "must be a 64-character hex pubkey",
                    ))
                }
            })
            .transpose()?;
        Ok(payload)
    }
}

/// One working interval, `start..end` in minutes after midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "IntervalWire", into = "IntervalWire")]
pub struct WorkInterval {
    /// Start, minutes after midnight.
    pub start: u16,
    /// End (exclusive), minutes after midnight, at most 1440.
    pub end: u16,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IntervalWire {
    start: String,
    end: String,
}

impl TryFrom<IntervalWire> for WorkInterval {
    type Error = String;

    fn try_from(wire: IntervalWire) -> std::result::Result<Self, String> {
        Ok(Self {
            start: parse_clock(&wire.start)?,
            end: parse_clock(&wire.end)?,
        })
    }
}

impl From<WorkInterval> for IntervalWire {
    fn from(interval: WorkInterval) -> Self {
        Self {
            start: format_clock(interval.start),
            end: format_clock(interval.end),
        }
    }
}

/// `HH:MM` (00:00–24:00) as minutes after midnight.
pub fn parse_clock(raw: &str) -> std::result::Result<u16, String> {
    let bad = || format!("{raw:?} is not a HH:MM time");
    let (hours, minutes) = raw.split_once(':').ok_or_else(bad)?;
    if hours.len() != 2 || minutes.len() != 2 {
        return Err(bad());
    }
    let hours: u16 = hours.parse().map_err(|_| bad())?;
    let minutes: u16 = minutes.parse().map_err(|_| bad())?;
    let total = hours * 60 + minutes;
    if minutes >= 60 || total > DAY_MINUTES {
        return Err(bad());
    }
    Ok(total)
}

/// Minutes after midnight as `HH:MM`.
pub fn format_clock(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// The working hours of one calendar day; no intervals is a day off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleDay {
    /// Calendar date `YYYY-MM-DD` in the space timezone.
    pub date: String,
    /// Working intervals in time order.
    #[serde(default)]
    pub intervals: Vec<WorkInterval>,
}

/// Content of a kind 42132 event: set the given days of one staff member.
/// Days not listed keep their current hours.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulePayload {
    /// Schema version, always 1.
    pub v: u64,
    /// Staff member whose days these are.
    pub staff_id: Uuid,
    /// The days to set, each at most once.
    pub days: Vec<ScheduleDay>,
}

impl SchedulePayload {
    /// Parse and validate kind 42132 content.
    pub fn parse(content: &str) -> Result<Self> {
        let mut payload: SchedulePayload = parse_versioned(content)?;
        if payload.days.is_empty() || payload.days.len() > MAX_SCHEDULE_DAYS {
            return Err(CrmError::field(
                "days",
                format!("must set 1–{MAX_SCHEDULE_DAYS} days"),
            ));
        }
        let mut seen = HashSet::new();
        for day in &mut payload.days {
            check_date(&day.date)?;
            if !seen.insert(day.date.clone()) {
                return Err(CrmError::field(
                    "days",
                    format!("{} is listed twice", day.date),
                ));
            }
            check_intervals(&mut day.intervals)?;
        }
        Ok(payload)
    }
}

fn check_intervals(intervals: &mut [WorkInterval]) -> Result<()> {
    if intervals.len() > MAX_DAY_INTERVALS {
        return Err(CrmError::field(
            "intervals",
            format!("at most {MAX_DAY_INTERVALS} intervals per day"),
        ));
    }
    intervals.sort_by_key(|interval| interval.start);
    let mut previous_end = 0;
    for interval in intervals.iter() {
        let on_grid =
            interval.start % SCHEDULE_STEP_MIN == 0 && interval.end % SCHEDULE_STEP_MIN == 0;
        if !on_grid || interval.start >= interval.end {
            return Err(CrmError::field(
                "intervals",
                format!(
                    "{}–{} must be a non-empty range on a {SCHEDULE_STEP_MIN}-minute grid",
                    format_clock(interval.start),
                    format_clock(interval.end)
                ),
            ));
        }
        if interval.start < previous_end {
            return Err(CrmError::field("intervals", "intervals must not overlap"));
        }
        previous_end = interval.end;
    }
    Ok(())
}

/// Validate a `YYYY-MM-DD` calendar date (years 2000–2999).
pub fn check_date(raw: &str) -> Result<()> {
    let bad = || CrmError::field("date", format!("{raw:?} is not a YYYY-MM-DD date"));
    let bytes = raw.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(bad());
    }
    let number = |range: std::ops::Range<usize>| -> Result<u32> {
        let part = &raw[range];
        if !part.chars().all(|c| c.is_ascii_digit()) {
            return Err(bad());
        }
        part.parse().map_err(|_| bad())
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(bad()),
    };
    if !(2000..=2999).contains(&year) || day == 0 || day > days_in_month {
        return Err(bad());
    }
    Ok(())
}

fn trimmed(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SERVICE: &str = "7d1b6c8e-1a2b-4c3d-8e9f-0a1b2c3d4e5f";

    fn staff() -> serde_json::Value {
        json!({
            "v": 1,
            "name": " Анна ",
            "position": " Барбер ",
            "phone": "+7 900 000-00-01",
            "service_ids": [SERVICE, SERVICE],
            "member": "AB".repeat(32)
        })
    }

    #[test]
    fn staff_is_normalized() {
        let staff = StaffPayload::parse(&staff().to_string()).unwrap();
        assert_eq!(staff.name, "Анна");
        assert_eq!(staff.position.as_deref(), Some("Барбер"));
        assert_eq!(staff.service_ids.len(), 1);
        assert_eq!(staff.member.unwrap(), "ab".repeat(32));
        assert!(!staff.archived);
    }

    #[test]
    fn invalid_staff_is_rejected() {
        let cases = [
            ("name", json!("")),
            ("position", json!("x".repeat(MAX_POSITION_CHARS + 1))),
            ("phone", json!("12")),
            ("member", json!("not-a-key")),
            ("service_ids", json!(["nope"])),
            ("extra", json!(1)),
        ];
        for (field, value) in cases {
            let mut payload = staff();
            payload[field] = value.clone();
            assert!(
                StaffPayload::parse(&payload.to_string()).is_err(),
                "{field} = {value}"
            );
        }
    }

    fn schedule(days: serde_json::Value) -> Result<SchedulePayload> {
        SchedulePayload::parse(&json!({"v": 1, "staff_id": SERVICE, "days": days}).to_string())
    }

    #[test]
    fn schedule_days_sort_their_intervals() {
        let payload = schedule(json!([
            {"date": "2026-10-26", "intervals": [
                {"start": "15:00", "end": "24:00"},
                {"start": "10:00", "end": "14:00"}
            ]},
            {"date": "2026-10-27", "intervals": []}
        ]))
        .unwrap();
        assert_eq!(
            payload.days[0].intervals,
            vec![
                WorkInterval {
                    start: 600,
                    end: 840
                },
                WorkInterval {
                    start: 900,
                    end: 1440
                }
            ]
        );
        assert!(payload.days[1].intervals.is_empty());
        let wire = serde_json::to_value(&payload).unwrap();
        assert_eq!(wire["days"][0]["intervals"][1]["end"], "24:00");
    }

    #[test]
    fn invalid_schedules_are_rejected() {
        let day = |date: &str, start: &str, end: &str| json!([{"date": date, "intervals": [{"start": start, "end": end}]}]);
        for days in [
            json!([]),
            day("2026-02-29", "10:00", "18:00"),
            day("2026-13-01", "10:00", "18:00"),
            day("26-10-01", "10:00", "18:00"),
            day("2026-10-26", "18:00", "10:00"),
            day("2026-10-26", "10:03", "18:00"),
            day("2026-10-26", "10:00", "24:05"),
            day("2026-10-26", "9:00", "18:00"),
            json!([
                {"date": "2026-10-26", "intervals": [{"start": "10:00", "end": "14:00"}, {"start": "13:00", "end": "18:00"}]}
            ]),
            json!([{"date": "2026-10-26"}, {"date": "2026-10-26"}]),
        ] {
            assert!(schedule(days.clone()).is_err(), "{days}");
        }
        assert!(check_date("2028-02-29").is_ok());
        assert!(check_date("2100-02-29").is_err());
    }

    #[test]
    fn clock_round_trips() {
        for minutes in [0, 5, 600, 1435, 1440] {
            assert_eq!(parse_clock(&format_clock(minutes)).unwrap(), minutes);
        }
    }
}
