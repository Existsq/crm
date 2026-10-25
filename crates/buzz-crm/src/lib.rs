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
//! - [`authz`] — the role × action table (ADR-0002).
//! - [`space`] — CRM space and warehouse payloads (kinds 42100, 42101).
//! - [`catalog`] — product payload, units, fiscal item types (kind 42102).
//! - [`party`] — customers and suppliers (kind 42103).
//! - [`sale`] — sale documents and their reversal (kinds 42104, 42105).
//! - [`tags`] — tag conventions shared by every CRM event.
//! - [`validate`] — names, INN, timezone.

pub mod authz;
pub mod catalog;
pub mod error;
pub mod money;
pub mod party;
pub mod sale;
pub mod space;
pub mod tags;
pub mod validate;
pub mod vat;

pub use authz::{capabilities, decide, CrmAction, CrmRole};
pub use catalog::{PaymentObject, ProductPayload, Unit};
pub use error::{CrmError, Result};
pub use money::{line_amount, Money, Qty};
pub use party::{PartyPayload, PartyType};
pub use sale::{PaymentMethod, SaleLine, SalePayload, SaleVoidPayload};
pub use space::{
    Currency, FiscalizationMode, SpacePayload, SpaceSettings, WarehousePayload, WarehouseSeed,
};
pub use vat::VatRate;
