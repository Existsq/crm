//! DTO10 JSON task format (`/api/v2/requests`) and the receipt mapping.
//!
//! Field names follow the ATOL DTO10 JSON task documentation. Names that are
//! only known from documentation — the 22 % VAT names, FFD 1.2 measure units,
//! the answer to a repeated UUID — must be confirmed once on a real device
//! (ADR-0003, "Последствия").

use buzz_crm::{FiscalParams, PaymentMethod, PaymentObject, Unit, VatRate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

/// Namespace for deterministic task UUIDs (`UUIDv5(namespace, "sell:<sale>")`).
const TASK_NAMESPACE: Uuid = Uuid::from_u128(0x6f0b_2c1e_8d3a_4f5b_9c7e_1a2b_3c4d_5e6f);

/// The UUID of the device task printing `operation` for `sale_id`.
///
/// Attempt 0 is the ADR-0003 `UUIDv5("sell:" + doc_id)`; a retry after the
/// device definitively refused the task (nothing printed) uses a new attempt.
pub fn task_uuid(operation: &str, sale_id: Uuid, attempt: u32) -> Uuid {
    let name = if attempt == 0 {
        format!("{operation}:{sale_id}")
    } else {
        format!("{operation}:{sale_id}:{attempt}")
    };
    Uuid::new_v5(&TASK_NAMESPACE, name.as_bytes())
}

/// The cashier named on receipts and reports (тег 1021).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operator {
    /// Full name.
    pub name: String,
    /// Cashier INN (тег 1203), optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vatin: Option<String>,
}

/// One receipt position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptLine {
    /// Name as printed.
    pub name: String,
    /// Price per unit, kopecks.
    pub price_minor: i64,
    /// Quantity, thousandths.
    pub qty_milli: i64,
    /// Line amount after discount, kopecks.
    pub amount_minor: i64,
    /// VAT rate.
    pub vat: VatRate,
    /// Unit of measure.
    pub unit: Unit,
    /// Fiscal item type.
    pub payment_object: PaymentObject,
}

/// One payment of a receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptPayment {
    /// Method.
    pub method: PaymentMethod,
    /// Amount, kopecks. For cash, the amount handed over (the device prints
    /// the change).
    pub amount_minor: i64,
}

/// A sale or return receipt to print.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptInput {
    /// `false`: sale (`sell`); `true`: return (`sellReturn`).
    pub is_return: bool,
    /// Positions.
    pub lines: Vec<ReceiptLine>,
    /// Payments.
    pub payments: Vec<ReceiptPayment>,
    /// Receipt total, kopecks.
    pub total_minor: i64,
    /// Customer phone or email for an electronic receipt (тег 1008).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_contact: Option<String>,
}

/// A DTO10 task.
#[derive(Debug, Clone, PartialEq)]
pub enum Task {
    /// `getShiftStatus`.
    ShiftStatus,
    /// `openShift`.
    OpenShift(Operator),
    /// `closeShift` (Z-report).
    CloseShift(Operator),
    /// `reportX`.
    ReportX(Operator),
    /// `sell` / `sellReturn`.
    Receipt(Operator, ReceiptInput),
}

fn rubles(minor: i64) -> Value {
    // DTO10 takes money as decimal numbers; kopecks / 100 is exact enough
    // for two decimals in an f64 and serializes as e.g. 89.9.
    json!(minor as f64 / 100.0)
}

fn quantity(milli: i64) -> Value {
    json!(milli as f64 / 1000.0)
}

/// FFD 1.2 measure unit names of DTO10 (`measurementUnit`).
pub fn measurement_unit(unit: Unit) -> &'static str {
    match unit {
        Unit::Piece => "piece",
        Unit::Kilogram => "kilogram",
        Unit::Gram => "gram",
        Unit::Liter => "liter",
        Unit::Milliliter => "milliliter",
        Unit::Meter => "meter",
        Unit::SquareMeter => "squareMeter",
        Unit::Hour => "hour",
        Unit::ServiceUnit => "otherUnits",
    }
}

/// DTO10 payment type: cash or electronic (card and transfer).
pub fn payment_type(method: PaymentMethod) -> &'static str {
    match method {
        PaymentMethod::Cash => "cash",
        PaymentMethod::Card | PaymentMethod::Transfer => "electronically",
    }
}

impl Task {
    /// The task JSON inside `request: [...]`.
    pub fn to_json(&self) -> Value {
        let operator = |op: &Operator| serde_json::to_value(op).unwrap_or(Value::Null);
        match self {
            Task::ShiftStatus => json!({"type": "getShiftStatus"}),
            Task::OpenShift(op) => json!({"type": "openShift", "operator": operator(op)}),
            Task::CloseShift(op) => json!({"type": "closeShift", "operator": operator(op)}),
            Task::ReportX(op) => json!({"type": "reportX", "operator": operator(op)}),
            Task::Receipt(op, receipt) => {
                let items: Vec<Value> = receipt
                    .lines
                    .iter()
                    .map(|line| {
                        json!({
                            "type": "position",
                            "name": line.name,
                            "price": rubles(line.price_minor),
                            "quantity": quantity(line.qty_milli),
                            "amount": rubles(line.amount_minor),
                            "measurementUnit": measurement_unit(line.unit),
                            "paymentMethod": "fullPayment",
                            "paymentObject": line.payment_object.as_str(),
                            "tax": {"type": line.vat.as_str()},
                        })
                    })
                    .collect();
                let mut by_type: Vec<(&str, i64)> = Vec::new();
                for payment in &receipt.payments {
                    let kind = payment_type(payment.method);
                    match by_type.iter_mut().find(|(t, _)| *t == kind) {
                        Some((_, sum)) => *sum += payment.amount_minor,
                        None => by_type.push((kind, payment.amount_minor)),
                    }
                }
                let payments: Vec<Value> = by_type
                    .into_iter()
                    .map(|(kind, sum)| json!({"type": kind, "sum": rubles(sum)}))
                    .collect();
                let mut task = json!({
                    "type": if receipt.is_return { "sellReturn" } else { "sell" },
                    "operator": operator(op),
                    "items": items,
                    "payments": payments,
                    "total": rubles(receipt.total_minor),
                });
                if let Some(contact) = &receipt.client_contact {
                    task["clientInfo"] = json!({"emailOrPhone": contact});
                }
                task
            }
        }
    }
}

/// Shift state reported by `getShiftStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShiftStatus {
    /// `opened`, `closed` or `expired` (open for more than 24 hours).
    pub state: String,
    /// Current (or last) shift number.
    pub number: u64,
}

/// Parse a `getShiftStatus` result.
pub fn parse_shift_status(result: &Value) -> Option<ShiftStatus> {
    let status = &result["shiftStatus"];
    Some(ShiftStatus {
        state: status["state"].as_str()?.to_owned(),
        number: status["number"].as_u64().unwrap_or(0),
    })
}

/// Parse `fiscalParams` of a printed document. `receipt_number` is 0 for
/// shift reports.
pub fn parse_fiscal_params(result: &Value) -> Option<FiscalParams> {
    let params = &result["fiscalParams"];
    let text = |key: &str| -> Option<String> {
        match &params[key] {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        }
    };
    Some(FiscalParams {
        document_number: params["fiscalDocumentNumber"].as_u64()?,
        document_sign: text("fiscalDocumentSign")?,
        receipt_number: params["fiscalReceiptNumber"].as_u64().unwrap_or(0),
        shift_number: params["shiftNumber"].as_u64()?,
        fn_number: text("fnNumber")?,
        registration_number: text("registrationNumber"),
        document_datetime: text("fiscalDocumentDateTime")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operator() -> Operator {
        Operator {
            name: "Иванова М.".into(),
            vatin: None,
        }
    }

    #[test]
    fn task_uuids_are_deterministic_per_attempt() {
        let sale = Uuid::nil();
        assert_eq!(task_uuid("sell", sale, 0), task_uuid("sell", sale, 0));
        assert_ne!(task_uuid("sell", sale, 0), task_uuid("sell", sale, 1));
        assert_ne!(
            task_uuid("sell", sale, 0),
            task_uuid("sell_return", sale, 0)
        );
        assert_eq!(task_uuid("sell", sale, 0).get_version_num(), 5);
    }

    #[test]
    fn receipt_task_matches_dto10_shape() {
        let task = Task::Receipt(
            operator(),
            ReceiptInput {
                is_return: false,
                lines: vec![ReceiptLine {
                    name: "Сыр".into(),
                    price_minor: 59_990,
                    qty_milli: 1_250,
                    amount_minor: 74_988,
                    vat: VatRate::Vat10,
                    unit: Unit::Kilogram,
                    payment_object: PaymentObject::Commodity,
                }],
                payments: vec![
                    ReceiptPayment {
                        method: PaymentMethod::Cash,
                        amount_minor: 50_000,
                    },
                    ReceiptPayment {
                        method: PaymentMethod::Card,
                        amount_minor: 20_000,
                    },
                    ReceiptPayment {
                        method: PaymentMethod::Transfer,
                        amount_minor: 4_988,
                    },
                ],
                total_minor: 74_988,
                client_contact: Some("+79120001122".into()),
            },
        )
        .to_json();
        assert_eq!(task["type"], "sell");
        assert_eq!(task["operator"]["name"], "Иванова М.");
        assert_eq!(task["items"][0]["price"], json!(599.9));
        assert_eq!(task["items"][0]["quantity"], json!(1.25));
        assert_eq!(task["items"][0]["amount"], json!(749.88));
        assert_eq!(task["items"][0]["measurementUnit"], "kilogram");
        assert_eq!(task["items"][0]["tax"]["type"], "vat10");
        // Card and transfer are both electronic payments on the receipt.
        assert_eq!(
            task["payments"],
            json!([{"type": "cash", "sum": 500.0}, {"type": "electronically", "sum": 249.88}])
        );
        assert_eq!(task["clientInfo"]["emailOrPhone"], "+79120001122");
    }

    #[test]
    fn fiscal_params_parse_numbers_and_strings() {
        let result = json!({"fiscalParams": {
            "fiscalDocumentNumber": 17, "fiscalDocumentSign": "3826380862",
            "fiscalReceiptNumber": 4, "shiftNumber": 2, "fnNumber": "9999078900004792",
            "registrationNumber": "0000000001002292",
            "fiscalDocumentDateTime": "2026-10-25T14:03:00+03:00", "total": 749.88
        }});
        let params = parse_fiscal_params(&result).unwrap();
        assert_eq!((params.document_number, params.receipt_number), (17, 4));
        assert_eq!(params.fn_number, "9999078900004792");
        assert!(parse_fiscal_params(&json!({})).is_none());
        let shift = parse_shift_status(&json!({"shiftStatus": {"state": "opened", "number": 3}}));
        assert_eq!(shift.unwrap().number, 3);
    }
}
