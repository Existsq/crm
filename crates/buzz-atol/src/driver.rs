//! Moves receipt jobs through print → record (ADR-0003).
//!
//! `pending` → submit the deterministic task UUID (a resubmit after a crash
//! gets 409 and resumes the same task) → `ready` stores the fiscal attributes
//! (`printed`) → the relay records kind 42120 → the job is deleted. A device
//! refusal is `failed` (retry with a new UUID); `blocked` or a task the
//! driver no longer knows is `needs_attention` — never retried on its own.

use std::future::Future;
use std::time::Duration;

use buzz_crm::{FiscalOperation, FiscalParams, FiscalReceiptPayload, Money};

use crate::client::{AtolClient, TaskState};
use crate::outbox::{Job, JobState, Outbox, OutboxError};
use crate::wire::{parse_fiscal_params, task_uuid, Operator, Task};

/// How long one pass waits for the device to finish printing.
pub const PRINT_TIMEOUT: Duration = Duration::from_secs(30);

/// What recording the receipt at the relay did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordOutcome {
    /// Recorded now.
    Recorded,
    /// The relay already had it (an earlier attempt's response was lost).
    AlreadyRecorded,
    /// Not recorded; keep the job and show the reason.
    Failed(String),
}

/// The kind 42120 payload for a printed job.
pub fn receipt_payload(job: &Job, fiscal: FiscalParams) -> FiscalReceiptPayload {
    FiscalReceiptPayload {
        v: 1,
        operation: if job.operation == "sell_return" {
            FiscalOperation::SellReturn
        } else {
            FiscalOperation::Sell
        },
        total_minor: Money::from_minor(job.receipt.total_minor).unwrap_or(Money::ZERO),
        task_uuid: task_uuid(&job.operation, job.sale_id, job.attempt).to_string(),
        fiscal,
    }
}

/// Advance one job as far as it can go now; returns its state afterwards
/// (`None` once it is done and removed).
pub async fn process<F, Fut>(
    outbox: &Outbox,
    client: &AtolClient,
    operator: &Operator,
    job: Job,
    record: F,
) -> Result<Option<JobState>, OutboxError>
where
    F: FnOnce(Job, FiscalParams) -> Fut,
    Fut: Future<Output = RecordOutcome>,
{
    let mut job = job;
    if job.state == JobState::Pending {
        let uuid = task_uuid(&job.operation, job.sale_id, job.attempt);
        let task = Task::Receipt(operator.clone(), job.receipt.clone());
        let pending_error = |error: &str| {
            outbox.set_state(
                &job.operation,
                job.sale_id,
                JobState::Pending,
                None,
                Some(error),
            )
        };
        if let Err(error) = client.submit(uuid, &task).await {
            // Whether the POST reached the device is unknown; the next pass
            // resubmits the same UUID and learns the answer.
            pending_error(&error.to_string())?;
            return Ok(Some(JobState::Pending));
        }
        match client.wait(uuid, PRINT_TIMEOUT).await {
            Err(error) => {
                pending_error(&error.to_string())?;
                return Ok(Some(JobState::Pending));
            }
            Ok(TaskState::Pending) => {
                pending_error("ККМ ещё печатает чек")?;
                return Ok(Some(JobState::Pending));
            }
            Ok(TaskState::Failed { code, description }) => {
                let error = format!("ККМ отказала ({code}): {description}");
                outbox.set_state(
                    &job.operation,
                    job.sale_id,
                    JobState::Failed,
                    None,
                    Some(&error),
                )?;
                return Ok(Some(JobState::Failed));
            }
            Ok(TaskState::Ambiguous(status)) => {
                let error =
                    format!("Неизвестно, напечатан ли чек (статус {status}). Проверьте ККМ.");
                outbox.set_state(
                    &job.operation,
                    job.sale_id,
                    JobState::NeedsAttention,
                    None,
                    Some(&error),
                )?;
                return Ok(Some(JobState::NeedsAttention));
            }
            Ok(TaskState::Unknown) => {
                let error = "ККМ не знает это задание. Проверьте, напечатан ли чек.";
                outbox.set_state(
                    &job.operation,
                    job.sale_id,
                    JobState::NeedsAttention,
                    None,
                    Some(error),
                )?;
                return Ok(Some(JobState::NeedsAttention));
            }
            Ok(TaskState::Ready(result)) => {
                outbox.set_state(
                    &job.operation,
                    job.sale_id,
                    JobState::Printed,
                    Some(&result),
                    None,
                )?;
                job.state = JobState::Printed;
                job.result = Some(result);
            }
        }
    }
    if job.state != JobState::Printed {
        return Ok(Some(job.state));
    }
    let Some(fiscal) = job.result.as_ref().and_then(parse_fiscal_params) else {
        let error = "ККМ вернула чек без фискальных реквизитов. Проверьте ККМ.";
        outbox.set_state(
            &job.operation,
            job.sale_id,
            JobState::NeedsAttention,
            None,
            Some(error),
        )?;
        return Ok(Some(JobState::NeedsAttention));
    };
    let (operation, sale_id) = (job.operation.clone(), job.sale_id);
    match record(job, fiscal).await {
        RecordOutcome::Recorded | RecordOutcome::AlreadyRecorded => {
            outbox.remove(&operation, sale_id)?;
            Ok(None)
        }
        RecordOutcome::Failed(error) => {
            outbox.set_state(&operation, sale_id, JobState::Printed, None, Some(&error))?;
            Ok(Some(JobState::Printed))
        }
    }
}
