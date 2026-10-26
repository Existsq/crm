#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Pure CRM/retail domain code for the Buzz CRM fork.
//!
//! Shared by the relay (validation and authorization inside the write
//! transaction), the SDK and CLI (event builders), and the desktop app.
//! No I/O lives here. See `VISION_CRM.md` and `docs/crm/adr/`.
//!
//! - [`money`] — integer kopecks and thousandth quantities (AD-004).
//! - [`vat`] — VAT rates as a product attribute.
//! - [`appointment`] — appointments with staff (kind 42133).
//! - [`authz`] — the role × action table (ADR-0002).
//! - [`space`] — CRM space and warehouse payloads (kinds 42100, 42101).
//! - [`catalog`] — product payload, units, fiscal item types (kind 42102).
//! - [`fiscal`] — fiscal receipts and cash-register shifts (kinds 42120, 42121).
//! - [`party`] — customers and suppliers (kind 42103).
//! - [`reminder`] — visit reminder rules and Telegram links (kinds 42150, 42151).
//! - [`sale`] — sale documents and their reversal (kinds 42104, 42105).
//! - [`staff`] — staff (masters) and their working days (kinds 42131, 42132).
//! - [`stock`] — stock documents, tech cards and moving-average cost
//!   (kinds 42106–42108).
//! - [`tags`] — tag conventions shared by every CRM event.
//! - [`validate`] — names, INN, timezone.

pub mod appointment;
pub mod authz;
pub mod catalog;
pub mod error;
pub mod fiscal;
pub mod money;
pub mod party;
pub mod reminder;
pub mod sale;
pub mod space;
pub mod staff;
pub mod stock;
pub mod tags;
pub mod validate;
pub mod vat;

pub use appointment::{
    AppointmentPayload, AppointmentService, AppointmentSource, AppointmentStatus,
};
pub use authz::{capabilities, decide, CrmAction, CrmRole};
pub use catalog::{PaymentObject, ProductPayload, Unit};
pub use error::{CrmError, Result};
pub use fiscal::{
    CashShiftPayload, FiscalOperation, FiscalParams, FiscalReceiptPayload, ShiftAction,
};
pub use money::{line_amount, Money, Qty};
pub use party::{PartyPayload, PartyType};
pub use reminder::{
    ClientTelegramPayload, ReminderContext, ReminderRule, ReminderSettingsPayload, ReminderTrigger,
};
pub use sale::{PaymentMethod, SaleLine, SalePayload, SalePayment, SaleVoidPayload};
pub use space::{
    Currency, FiscalizationMode, SpacePayload, SpaceSettings, WarehousePayload, WarehouseSeed,
};
pub use staff::{
    format_clock, parse_clock, ScheduleDay, SchedulePayload, StaffPayload, WorkInterval,
};
pub use stock::{
    StockDelta, StockDocLine, StockDocPayload, StockDocType, StockLevel, TechCardItem,
    TechCardPayload,
};
pub use vat::VatRate;
