//! Durable receipt queue (SQLite) for the desktop.
//!
//! A job is written before the device is asked to print and is deleted only
//! after the relay has recorded the fiscal receipt (kind 42120), so a crash or
//! lost connection at any step leaves a row to resume from — never a second
//! receipt (ADR-0003).

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::wire::ReceiptInput;

/// Where a job stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Not yet confirmed printed: submit (or resume) the device task.
    Pending,
    /// Printed; the fiscal attributes are stored and wait to reach the relay.
    Printed,
    /// The device refused the task (nothing printed); a retry uses a new
    /// task UUID.
    Failed,
    /// Unknown whether it printed: a person must check the device.
    NeedsAttention,
}

impl JobState {
    fn as_str(self) -> &'static str {
        match self {
            JobState::Pending => "pending",
            JobState::Printed => "printed",
            JobState::Failed => "failed",
            JobState::NeedsAttention => "needs_attention",
        }
    }

    fn parse(raw: &str) -> Self {
        match raw {
            "printed" => JobState::Printed,
            "failed" => JobState::Failed,
            "needs_attention" => JobState::NeedsAttention,
            _ => JobState::Pending,
        }
    }
}

/// One receipt to print and record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// `sell` or `sell_return`.
    pub operation: String,
    /// Sale the receipt is for.
    pub sale_id: Uuid,
    /// CRM space channel the receipt event goes to.
    pub channel_id: Uuid,
    /// Sale number, for display.
    pub sale_number: i64,
    /// The receipt.
    pub receipt: ReceiptInput,
    /// Retry counter; part of the task UUID.
    pub attempt: u32,
    /// State.
    pub state: JobState,
    /// Device result once printed (`fiscalParams` JSON).
    pub result: Option<serde_json::Value>,
    /// Last error text for the cashier.
    pub error: Option<String>,
    /// Unix seconds of the last change.
    pub updated_at: i64,
}

/// Outbox errors.
#[derive(Debug, thiserror::Error)]
pub enum OutboxError {
    /// SQLite failed.
    #[error("receipt queue: {0}")]
    Sql(#[from] rusqlite::Error),
    /// A stored row could not be decoded.
    #[error("receipt queue row: {0}")]
    Decode(#[from] serde_json::Error),
}

/// The SQLite-backed queue.
pub struct Outbox {
    conn: Mutex<Connection>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

impl Outbox {
    /// Open (or create) the queue at `path`.
    pub fn open(path: &Path) -> Result<Self, OutboxError> {
        Self::init(Connection::open(path)?)
    }

    /// An in-memory queue (tests).
    pub fn in_memory() -> Result<Self, OutboxError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, OutboxError> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             CREATE TABLE IF NOT EXISTS receipt_jobs (
                 operation   TEXT    NOT NULL,
                 sale_id     TEXT    NOT NULL,
                 channel_id  TEXT    NOT NULL,
                 sale_number INTEGER NOT NULL,
                 receipt     TEXT    NOT NULL,
                 attempt     INTEGER NOT NULL DEFAULT 0,
                 state       TEXT    NOT NULL DEFAULT 'pending',
                 result      TEXT,
                 error       TEXT,
                 updated_at  INTEGER NOT NULL,
                 PRIMARY KEY (operation, sale_id)
             );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Queue a receipt. Queuing the same `(operation, sale)` again keeps the
    /// existing job, so a double click never prints twice.
    pub fn enqueue(
        &self,
        operation: &str,
        sale_id: Uuid,
        channel_id: Uuid,
        sale_number: i64,
        receipt: &ReceiptInput,
    ) -> Result<Job, OutboxError> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO receipt_jobs
                 (operation, sale_id, channel_id, sale_number, receipt, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (operation, sale_id) DO NOTHING",
            params![
                operation,
                sale_id.to_string(),
                channel_id.to_string(),
                sale_number,
                serde_json::to_string(receipt)?,
                now()
            ],
        )?;
        drop(conn);
        self.get(operation, sale_id)?
            .ok_or(OutboxError::Sql(rusqlite::Error::QueryReturnedNoRows))
    }

    /// One job.
    pub fn get(&self, operation: &str, sale_id: Uuid) -> Result<Option<Job>, OutboxError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT operation, sale_id, channel_id, sale_number, receipt, attempt, state,
                        result, error, updated_at
                 FROM receipt_jobs WHERE operation = ?1 AND sale_id = ?2",
                params![operation, sale_id.to_string()],
                raw_row,
            )
            .optional()?;
        row.map(decode).transpose()
    }

    /// Every job, oldest change first.
    pub fn list(&self) -> Result<Vec<Job>, OutboxError> {
        let conn = self.conn();
        let mut statement = conn.prepare(
            "SELECT operation, sale_id, channel_id, sale_number, receipt, attempt, state,
                    result, error, updated_at
             FROM receipt_jobs ORDER BY updated_at, sale_number",
        )?;
        let rows = statement
            .query_map([], raw_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(decode).collect()
    }

    /// Move a job to `state` with an optional result and error.
    pub fn set_state(
        &self,
        operation: &str,
        sale_id: Uuid,
        state: JobState,
        result: Option<&serde_json::Value>,
        error: Option<&str>,
    ) -> Result<(), OutboxError> {
        let result = result.map(serde_json::to_string).transpose()?;
        self.conn().execute(
            "UPDATE receipt_jobs SET state = ?3, result = COALESCE(?4, result), error = ?5,
                                     updated_at = ?6
             WHERE operation = ?1 AND sale_id = ?2",
            params![
                operation,
                sale_id.to_string(),
                state.as_str(),
                result,
                error,
                now()
            ],
        )?;
        Ok(())
    }

    /// Prepare a failed job for another try with a fresh task UUID.
    pub fn retry(&self, operation: &str, sale_id: Uuid) -> Result<(), OutboxError> {
        self.conn().execute(
            "UPDATE receipt_jobs SET state = 'pending', attempt = attempt + 1, error = NULL,
                                     updated_at = ?3
             WHERE operation = ?1 AND sale_id = ?2 AND state = 'failed'",
            params![operation, sale_id.to_string(), now()],
        )?;
        Ok(())
    }

    /// Resume a needs-attention job with the same task UUID (the person
    /// checked: the device still knows the task).
    pub fn resume(&self, operation: &str, sale_id: Uuid) -> Result<(), OutboxError> {
        self.conn().execute(
            "UPDATE receipt_jobs SET state = 'pending', error = NULL, updated_at = ?3
             WHERE operation = ?1 AND sale_id = ?2 AND state = 'needs_attention'",
            params![operation, sale_id.to_string(), now()],
        )?;
        Ok(())
    }

    /// Delete a job: after the relay recorded the receipt, or when a person
    /// decided it must not be printed.
    pub fn remove(&self, operation: &str, sale_id: Uuid) -> Result<(), OutboxError> {
        self.conn().execute(
            "DELETE FROM receipt_jobs WHERE operation = ?1 AND sale_id = ?2",
            params![operation, sale_id.to_string()],
        )?;
        Ok(())
    }
}

type RawRow = (
    String,
    String,
    String,
    i64,
    String,
    u32,
    String,
    Option<String>,
    Option<String>,
    i64,
);

fn raw_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

fn decode(raw: RawRow) -> Result<Job, OutboxError> {
    let (operation, sale, channel, sale_number, receipt, attempt, state, result, error, updated) =
        raw;
    let uuid = |value: &str| {
        Uuid::parse_str(value).map_err(|e| OutboxError::Decode(serde::de::Error::custom(e)))
    };
    Ok(Job {
        operation,
        sale_id: uuid(&sale)?,
        channel_id: uuid(&channel)?,
        sale_number,
        receipt: serde_json::from_str(&receipt)?,
        attempt,
        state: JobState::parse(&state),
        result: result.as_deref().map(serde_json::from_str).transpose()?,
        error,
        updated_at: updated,
    })
}
