//! CRM appointments (migration 0056, kind 42133).
//!
//! Writes run inside the caller's event-write transaction (see the parent
//! module). The writer locks the staff row, so two bookings for one staff
//! member are checked against each other one at a time: an appointment that
//! holds its time never overlaps another one for the same staff member.

use buzz_datastore_tracing::datastore_span;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{PgPool, Postgres, Row as _, Transaction};
use uuid::Uuid;

use super::hex_bytes;
use crate::{error::Result, CommunityId, Db};

/// Most appointments one read returns (a busy salon books ~100 a day).
pub const MAX_APPOINTMENTS: i64 = 3000;
/// Widest `from..=to` range of an appointment read, in days.
pub const MAX_APPOINTMENT_RANGE_DAYS: i64 = 93;

/// Statuses that free the appointment's time, as an SQL list.
macro_rules! freeing_statuses {
    () => {
        "('no_show', 'cancelled')"
    };
}

/// One booked service as written.
#[derive(Debug, Clone, Copy)]
pub struct AppointmentServiceWrite {
    /// Bookable product.
    pub product_id: Uuid,
    /// Booked minutes.
    pub duration_min: i32,
    /// Agreed price, kopecks.
    pub price_minor: i64,
}

/// A validated kind 42133 payload ready to project.
#[derive(Debug, Clone, Copy)]
pub struct AppointmentWrite<'a> {
    /// Appointment id (`crm-id` tag).
    pub appointment_id: Uuid,
    /// CRM space channel.
    pub space_channel_id: Uuid,
    /// Staff member booked.
    pub staff_id: Uuid,
    /// Calendar day in the space timezone.
    pub day: NaiveDate,
    /// Start, minutes after midnight.
    pub start_min: i32,
    /// End (exclusive), minutes after midnight.
    pub end_min: i32,
    /// Services in order.
    pub services: &'a [AppointmentServiceWrite],
    /// Client card, if any.
    pub customer_id: Option<Uuid>,
    /// Client name without a card.
    pub client_name: Option<&'a str>,
    /// Client phone without a card.
    pub client_phone: Option<&'a str>,
    /// Wire status name.
    pub status: &'a str,
    /// Whether the status holds the appointment's time.
    pub holds_time: bool,
    /// Free-form note.
    pub note: Option<&'a str>,
    /// Wire source name.
    pub source: &'a str,
    /// Skip the working-hours check.
    pub outside_hours: bool,
    /// Event id of this revision.
    pub revision_event_id: &'a [u8],
    /// Author pubkey.
    pub actor: &'a [u8],
}

/// Outcome of an appointment write; anything but `Written` rolls back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppointmentWriteOutcome {
    /// Inserted or updated.
    Written,
    /// A create named an id that already exists.
    AlreadyExists,
    /// An update named an id that does not exist in this space.
    Missing,
    /// The appointment changed since `expected_revision`.
    RevisionMismatch,
    /// The staff member does not exist in this space.
    UnknownStaff,
    /// The staff member is archived.
    ArchivedStaff,
    /// A service is not a bookable product of this space (or is archived).
    UnknownService(Uuid),
    /// The staff member does not perform this service.
    ServiceNotOffered(Uuid),
    /// The customer is not a party of this space.
    UnknownCustomer,
    /// The visit is not inside the staff member's working hours.
    OutsideHours,
    /// The visit overlaps another appointment `start..end` of this staff member.
    Overlap {
        /// Start of the other appointment, minutes after midnight.
        start: i32,
        /// End of the other appointment, minutes after midnight.
        end: i32,
    },
    /// The appointment is paid; its staff, time and services are fixed.
    Paid,
}

/// Current appointment row as `GET /crm/appointments` returns it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrmAppointmentRecord {
    /// Appointment id.
    #[serde(rename = "id")]
    pub appointment_id: Uuid,
    /// Staff member booked.
    pub staff_id: Uuid,
    /// Calendar day.
    #[serde(rename = "date")]
    pub day: NaiveDate,
    /// Start, minutes after midnight.
    #[serde(rename = "start")]
    pub start_min: i32,
    /// End, minutes after midnight.
    #[serde(rename = "end")]
    pub end_min: i32,
    /// `[{"product_id", "name", "duration_min", "price_minor"}]`.
    pub services: Value,
    /// Sum of the service prices, kopecks.
    pub total_minor: i64,
    /// Client card.
    pub customer_id: Option<Uuid>,
    /// Name on the client card.
    pub customer_name: Option<String>,
    /// Phone on the client card.
    pub customer_phone: Option<String>,
    /// Client name without a card.
    pub client_name: Option<String>,
    /// Client phone without a card.
    pub client_phone: Option<String>,
    /// Wire status name.
    pub status: String,
    /// Note.
    pub note: Option<String>,
    /// Wire source name.
    pub source: String,
    /// Booked outside working hours.
    pub outside_hours: bool,
    /// Sale that paid for the visit.
    pub sale_id: Option<Uuid>,
    /// Current revision event id.
    #[serde(rename = "revision", serialize_with = "hex_bytes")]
    pub revision_event_id: Vec<u8>,
    /// First revision time.
    pub created_at: DateTime<Utc>,
    /// Latest revision time.
    pub updated_at: DateTime<Utc>,
}

/// Filters of an appointment read.
#[derive(Debug, Clone, Copy, Default)]
pub struct AppointmentQuery {
    /// First day.
    pub from: Option<NaiveDate>,
    /// Last day.
    pub to: Option<NaiveDate>,
    /// Only this staff member.
    pub staff_id: Option<Uuid>,
    /// Only this client card.
    pub customer_id: Option<Uuid>,
}

/// What the slot picker needs about one day of the space.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotInputs {
    /// Active staff who perform every requested service, with the day's
    /// working intervals (`[{"start", "end"}]`, empty when off or unset).
    pub staff: Vec<(Uuid, String, Value)>,
    /// Time held by appointments that day: `(staff_id, start, end)`.
    pub busy: Vec<(Uuid, i32, i32)>,
    /// Booked minutes and agreed prices of the requested services, in order:
    /// `(product_id, name, duration_min, price_minor)`; unknown or goods ids
    /// are left out.
    pub services: Vec<(Uuid, String, Option<i32>, i64)>,
    /// Current local date and time in the space timezone.
    pub now_local: NaiveDateTime,
}

/// Create (no `expected_revision`) or CAS-update an appointment.
pub async fn write_appointment(
    tx: &mut Transaction<'_, Postgres>,
    community: CommunityId,
    write: AppointmentWrite<'_>,
    expected_revision: Option<&[u8]>,
) -> Result<AppointmentWriteOutcome> {
    use AppointmentWriteOutcome as Out;
    let current = sqlx::query(
        "SELECT space_channel_id, revision_event_id, staff_id, day, start_min, end_min, \
                total_minor, sale_id \
         FROM crm_appointments WHERE community_id = $1 AND appointment_id = $2 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(write.appointment_id)
    .fetch_optional(&mut **tx)
    .await?;
    let total: i64 = write.services.iter().map(|s| s.price_minor).sum();
    match (expected_revision, current) {
        (None, Some(_)) => return Ok(Out::AlreadyExists),
        (Some(_), None) => return Ok(Out::Missing),
        (Some(expected), Some(row)) => {
            let space: Uuid = row.try_get("space_channel_id")?;
            if space != write.space_channel_id {
                return Ok(Out::Missing);
            }
            let revision: Vec<u8> = row.try_get("revision_event_id")?;
            if revision != expected {
                return Ok(Out::RevisionMismatch);
            }
            let sale: Option<Uuid> = row.try_get("sale_id")?;
            let moved = row.try_get::<Uuid, _>("staff_id")? != write.staff_id
                || row.try_get::<NaiveDate, _>("day")? != write.day
                || row.try_get::<i32, _>("start_min")? != write.start_min
                || row.try_get::<i32, _>("end_min")? != write.end_min
                || row.try_get::<i64, _>("total_minor")? != total;
            if sale.is_some() && moved {
                return Ok(Out::Paid);
            }
        }
        (None, None) => {}
    }

    // Exclusive lock: bookings of one staff member are checked one by one.
    let staff = sqlx::query(
        "SELECT archived_at IS NOT NULL AS archived, service_ids FROM crm_staff \
         WHERE community_id = $1 AND space_channel_id = $2 AND staff_id = $3 FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(write.space_channel_id)
    .bind(write.staff_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(staff) = staff else {
        return Ok(Out::UnknownStaff);
    };
    if write.holds_time && staff.try_get::<bool, _>("archived")? {
        return Ok(Out::ArchivedStaff);
    }
    let offered: Vec<Uuid> = staff.try_get("service_ids")?;

    let ids: Vec<Uuid> = write.services.iter().map(|s| s.product_id).collect();
    let rows = sqlx::query(
        "SELECT product_id, name, archived_at IS NOT NULL AS archived FROM crm_products \
         WHERE community_id = $1 AND space_channel_id = $2 AND product_id = ANY($3) \
           AND payment_object IN ('service', 'job') \
         FOR SHARE",
    )
    .bind(community.as_uuid())
    .bind(write.space_channel_id)
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    let mut names = Vec::with_capacity(ids.len());
    for service in write.services {
        let Some(row) = rows.iter().find(|row| {
            row.try_get::<Uuid, _>("product_id")
                .is_ok_and(|id| id == service.product_id)
        }) else {
            return Ok(Out::UnknownService(service.product_id));
        };
        if write.holds_time {
            if expected_revision.is_none() && row.try_get::<bool, _>("archived")? {
                return Ok(Out::UnknownService(service.product_id));
            }
            if !offered.contains(&service.product_id) {
                return Ok(Out::ServiceNotOffered(service.product_id));
            }
        }
        names.push(row.try_get::<String, _>("name")?);
    }

    if let Some(customer) = write.customer_id {
        let known: Option<Uuid> = sqlx::query_scalar(
            "SELECT party_id FROM crm_parties \
             WHERE community_id = $1 AND space_channel_id = $2 AND party_id = $3 FOR SHARE",
        )
        .bind(community.as_uuid())
        .bind(write.space_channel_id)
        .bind(customer)
        .fetch_optional(&mut **tx)
        .await?;
        if known.is_none() {
            return Ok(Out::UnknownCustomer);
        }
    }

    if write.holds_time {
        if !write.outside_hours {
            let intervals: Option<Value> = sqlx::query_scalar(
                "SELECT intervals FROM crm_staff_days \
                 WHERE community_id = $1 AND staff_id = $2 AND day = $3",
            )
            .bind(community.as_uuid())
            .bind(write.staff_id)
            .bind(write.day)
            .fetch_optional(&mut **tx)
            .await?;
            let inside = intervals
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(|list| {
                    list.iter().any(|interval| {
                        let start = interval["start"].as_i64().unwrap_or(i64::MAX);
                        let end = interval["end"].as_i64().unwrap_or(i64::MIN);
                        start <= i64::from(write.start_min) && i64::from(write.end_min) <= end
                    })
                });
            if !inside {
                return Ok(Out::OutsideHours);
            }
        }
        let clash = sqlx::query(concat!(
            "SELECT start_min, end_min FROM crm_appointments \
             WHERE community_id = $1 AND staff_id = $2 AND day = $3 \
               AND appointment_id <> $4 AND status NOT IN ",
            freeing_statuses!(),
            " \
               AND start_min < $6 AND $5 < end_min \
             ORDER BY start_min LIMIT 1"
        ))
        .bind(community.as_uuid())
        .bind(write.staff_id)
        .bind(write.day)
        .bind(write.appointment_id)
        .bind(write.start_min)
        .bind(write.end_min)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(row) = clash {
            return Ok(Out::Overlap {
                start: row.try_get("start_min")?,
                end: row.try_get("end_min")?,
            });
        }
    }

    let services: Value = write
        .services
        .iter()
        .zip(&names)
        .map(|(service, name)| {
            json!({
                "product_id": service.product_id,
                "name": name,
                "duration_min": service.duration_min,
                "price_minor": service.price_minor,
            })
        })
        .collect();
    let create = expected_revision.is_none();
    let query = sqlx::query(if create {
        "INSERT INTO crm_appointments \
             (community_id, appointment_id, space_channel_id, staff_id, day, start_min, end_min, \
              services, total_minor, customer_id, client_name, client_phone, status, note, \
              source, outside_hours, revision_event_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18) \
         ON CONFLICT (community_id, appointment_id) DO NOTHING"
    } else {
        "UPDATE crm_appointments SET \
             staff_id = $4, day = $5, start_min = $6, end_min = $7, services = $8, \
             total_minor = $9, customer_id = $10, client_name = $11, client_phone = $12, \
             status = $13, note = $14, source = $15, outside_hours = $16, \
             revision_event_id = $17, updated_at = now() \
         WHERE community_id = $1 AND appointment_id = $2 AND space_channel_id = $3"
    })
    .bind(community.as_uuid())
    .bind(write.appointment_id)
    .bind(write.space_channel_id)
    .bind(write.staff_id)
    .bind(write.day)
    .bind(write.start_min)
    .bind(write.end_min)
    .bind(&services)
    .bind(total)
    .bind(write.customer_id)
    .bind(write.client_name)
    .bind(write.client_phone)
    .bind(write.status)
    .bind(write.note)
    .bind(write.source)
    .bind(write.outside_hours)
    .bind(write.revision_event_id);
    let query = if create {
        query.bind(write.actor)
    } else {
        query
    };
    let written = query.execute(&mut **tx).await?;
    if written.rows_affected() == 1 {
        Ok(Out::Written)
    } else {
        Ok(Out::AlreadyExists)
    }
}

macro_rules! appointment_columns {
    () => {
        "a.appointment_id, a.staff_id, a.day, a.start_min, a.end_min, a.services, \
         a.total_minor, a.customer_id, p.name AS customer_name, p.phone AS customer_phone, \
         a.client_name, a.client_phone, a.status, a.note, a.source, a.outside_hours, \
         a.sale_id, a.revision_event_id, a.created_at, a.updated_at"
    };
}

/// Appointments of the space matching `query`, in `(day, start, staff)` order.
pub async fn list_appointments(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    query: &AppointmentQuery,
) -> Result<Vec<CrmAppointmentRecord>> {
    let sql = concat!(
        "SELECT ",
        appointment_columns!(),
        " FROM crm_appointments a \
         LEFT JOIN crm_parties p \
           ON p.community_id = a.community_id AND p.party_id = a.customer_id \
         WHERE a.community_id = $1 AND a.space_channel_id = $2 \
           AND ($3::date IS NULL OR a.day >= $3) AND ($4::date IS NULL OR a.day <= $4) \
           AND ($5::uuid IS NULL OR a.staff_id = $5) \
           AND ($6::uuid IS NULL OR a.customer_id = $6) \
         ORDER BY a.day, a.start_min, a.staff_id, a.appointment_id LIMIT $7"
    );
    sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(space_channel_id)
        .bind(query.from)
        .bind(query.to)
        .bind(query.staff_id)
        .bind(query.customer_id)
        .bind(MAX_APPOINTMENTS)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(appointment_from_row)
        .collect()
}

/// One appointment of the space.
pub async fn get_appointment(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    appointment_id: Uuid,
) -> Result<Option<CrmAppointmentRecord>> {
    let sql = concat!(
        "SELECT ",
        appointment_columns!(),
        " FROM crm_appointments a \
         LEFT JOIN crm_parties p \
           ON p.community_id = a.community_id AND p.party_id = a.customer_id \
         WHERE a.community_id = $1 AND a.space_channel_id = $2 AND a.appointment_id = $3"
    );
    sqlx::query(sql)
        .bind(community.as_uuid())
        .bind(space_channel_id)
        .bind(appointment_id)
        .fetch_optional(pool)
        .await?
        .map(appointment_from_row)
        .transpose()
}

/// Inputs of the slot picker for `day` and the requested services.
pub async fn slot_inputs(
    pool: &PgPool,
    community: CommunityId,
    space_channel_id: Uuid,
    day: NaiveDate,
    service_ids: &[Uuid],
    staff_id: Option<Uuid>,
) -> Result<Option<SlotInputs>> {
    let now_local: Option<NaiveDateTime> = sqlx::query_scalar(
        "SELECT (now() AT TIME ZONE (settings->>'timezone'))::timestamp FROM crm_spaces \
         WHERE community_id = $1 AND channel_id = $2",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .fetch_optional(pool)
    .await?;
    let Some(now_local) = now_local else {
        return Ok(None);
    };
    let services = sqlx::query(
        "SELECT p.product_id, p.name, p.duration_min, p.price_minor \
         FROM unnest($3::uuid[]) WITH ORDINALITY AS r(id, n) \
         JOIN crm_products p ON p.community_id = $1 AND p.space_channel_id = $2 \
           AND p.product_id = r.id AND p.payment_object IN ('service', 'job') \
           AND p.archived_at IS NULL \
         ORDER BY r.n",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(service_ids)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok((
            row.try_get("product_id")?,
            row.try_get("name")?,
            row.try_get("duration_min")?,
            row.try_get("price_minor")?,
        ))
    })
    .collect::<Result<Vec<_>>>()?;
    let staff = sqlx::query(
        "SELECT s.staff_id, s.name, COALESCE(d.intervals, '[]'::jsonb) AS intervals \
         FROM crm_staff s \
         LEFT JOIN crm_staff_days d \
           ON d.community_id = s.community_id AND d.staff_id = s.staff_id AND d.day = $3 \
         WHERE s.community_id = $1 AND s.space_channel_id = $2 AND s.archived_at IS NULL \
           AND s.service_ids @> $4::uuid[] AND ($5::uuid IS NULL OR s.staff_id = $5) \
         ORDER BY lower(s.name), s.staff_id",
    )
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(day)
    .bind(service_ids)
    .bind(staff_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok((
            row.try_get("staff_id")?,
            row.try_get("name")?,
            row.try_get("intervals")?,
        ))
    })
    .collect::<Result<Vec<_>>>()?;
    let busy = sqlx::query(concat!(
        "SELECT staff_id, start_min, end_min FROM crm_appointments \
         WHERE community_id = $1 AND space_channel_id = $2 AND day = $3 \
           AND status NOT IN ",
        freeing_statuses!(),
        " ORDER BY staff_id, start_min"
    ))
    .bind(community.as_uuid())
    .bind(space_channel_id)
    .bind(day)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok((
            row.try_get("staff_id")?,
            row.try_get("start_min")?,
            row.try_get("end_min")?,
        ))
    })
    .collect::<Result<Vec<_>>>()?;
    Ok(Some(SlotInputs {
        staff,
        busy,
        services,
        now_local,
    }))
}

fn appointment_from_row(row: sqlx::postgres::PgRow) -> Result<CrmAppointmentRecord> {
    Ok(CrmAppointmentRecord {
        appointment_id: row.try_get("appointment_id")?,
        staff_id: row.try_get("staff_id")?,
        day: row.try_get("day")?,
        start_min: row.try_get("start_min")?,
        end_min: row.try_get("end_min")?,
        services: row.try_get("services")?,
        total_minor: row.try_get("total_minor")?,
        customer_id: row.try_get("customer_id")?,
        customer_name: row.try_get("customer_name")?,
        customer_phone: row.try_get("customer_phone")?,
        client_name: row.try_get("client_name")?,
        client_phone: row.try_get("client_phone")?,
        status: row.try_get("status")?,
        note: row.try_get("note")?,
        source: row.try_get("source")?,
        outside_hours: row.try_get("outside_hours")?,
        sale_id: row.try_get("sale_id")?,
        revision_event_id: row.try_get("revision_event_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

impl Db {
    /// Appointments of the space.
    #[datastore_span(name = "crm_list_appointments", system = "postgresql")]
    pub async fn crm_list_appointments(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        query: &AppointmentQuery,
    ) -> Result<Vec<CrmAppointmentRecord>> {
        list_appointments(&self.pool, community, space_channel_id, query).await
    }

    /// One appointment.
    #[datastore_span(name = "crm_get_appointment", system = "postgresql")]
    pub async fn crm_get_appointment(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        appointment_id: Uuid,
    ) -> Result<Option<CrmAppointmentRecord>> {
        get_appointment(&self.pool, community, space_channel_id, appointment_id).await
    }

    /// Slot-picker inputs for one day; `None` when the space is gone.
    #[datastore_span(name = "crm_slot_inputs", system = "postgresql")]
    pub async fn crm_slot_inputs(
        &self,
        community: CommunityId,
        space_channel_id: Uuid,
        day: NaiveDate,
        service_ids: &[Uuid],
        staff_id: Option<Uuid>,
    ) -> Result<Option<SlotInputs>> {
        slot_inputs(
            &self.pool,
            community,
            space_channel_id,
            day,
            service_ids,
            staff_id,
        )
        .await
    }
}

#[cfg(test)]
mod postgres_tests;
