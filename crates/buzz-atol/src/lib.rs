#![deny(unsafe_code)]
#![warn(missing_docs)]
//! ATOL DTO10 integration for the CRM cash desk (ADR-0003).
//!
//! - [`wire`] — DTO10 JSON tasks and the receipt mapping.
//! - [`client`] — HTTP client with status polling.
//! - [`outbox`] — durable SQLite receipt queue.
//! - [`driver`] — print → record state machine over the queue.
//!
//! No Tauri here: the desktop wires these into commands, and the tests run
//! against `buzz-atol-emulator`.

pub mod client;
pub mod driver;
pub mod outbox;
pub mod wire;

pub use client::{AtolClient, AtolError, TaskState, DEFAULT_BASE_URL};
pub use driver::{process, receipt_payload, RecordOutcome};
pub use outbox::{Job, JobState, Outbox, OutboxError};
pub use wire::{Operator, ReceiptInput, ReceiptLine, ReceiptPayment, ShiftStatus, Task};
