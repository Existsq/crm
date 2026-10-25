//! CRM cash desk: the ATOL DTO10 cash register (ADR-0003).
//!
//! The desktop talks to the DTO10 web server (or `buzz-atol-emulator`) and
//! records what the device printed at the relay: cash shifts (kind 42121)
//! and fiscal receipts (kind 42120). Receipts go through the durable
//! `buzz-atol` outbox in the app data dir, so a crash, a lost connection or a
//! double click never prints a receipt twice.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use buzz_atol_pkg::{
    process, receipt_payload, wire::parse_fiscal_params, wire::parse_shift_status, AtolClient, Job,
    JobState, Operator, Outbox, ReceiptInput, ReceiptLine, ReceiptPayment, RecordOutcome, Task,
    TaskState, DEFAULT_BASE_URL,
};
use buzz_crm_pkg::{
    CashShiftPayload, FiscalParams, PaymentMethod, PaymentObject, ShiftAction, Unit, VatRate,
};

use super::crm::{parse_uuid, sdk_error};
use crate::{
    app_state::AppState,
    relay::{get_relay_json, submit_event},
};

/// How long a report or shift task may take on the device.
const REPORT_TIMEOUT: Duration = Duration::from_secs(30);

/// Cash-desk settings, stored next to the receipt queue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CashSettings {
    /// DTO10 web-server URL.
    pub base_url: String,
    /// Cashier name printed on receipts (тег 1021).
    pub operator_name: String,
}

impl Default for CashSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            operator_name: "Кассир".to_string(),
        }
    }
}

/// Lazily opened receipt queue and settings.
#[derive(Default)]
pub struct CashDeskState {
    outbox: Mutex<Option<Arc<Outbox>>>,
    /// Serializes queue passes, so two windows never drive one job at once.
    pass: tokio::sync::Mutex<()>,
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("resolve app data dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create app data dir: {e}"))?;
    Ok(dir)
}

fn outbox(app: &AppHandle, desk: &CashDeskState) -> Result<Arc<Outbox>, String> {
    let mut slot = desk.outbox.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(outbox) = slot.as_ref() {
        return Ok(outbox.clone());
    }
    let outbox = Arc::new(
        Outbox::open(&data_dir(app)?.join("crm-receipt-queue.db")).map_err(|e| e.to_string())?,
    );
    *slot = Some(outbox.clone());
    Ok(outbox)
}

fn load_settings(app: &AppHandle) -> CashSettings {
    data_dir(app)
        .ok()
        .and_then(|dir| std::fs::read_to_string(dir.join("crm-cash-desk.json")).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn client(settings: &CashSettings) -> Result<AtolClient, String> {
    AtolClient::new(&settings.base_url).map_err(|e| e.to_string())
}

fn operator(settings: &CashSettings) -> Operator {
    Operator {
        name: settings.operator_name.clone(),
        vatin: None,
    }
}

/// Cash-desk settings.
#[tauri::command]
pub fn cash_get_settings(app: AppHandle) -> CashSettings {
    load_settings(&app)
}

/// Save cash-desk settings.
#[tauri::command]
pub fn cash_set_settings(settings: CashSettings, app: AppHandle) -> Result<CashSettings, String> {
    let url = settings.base_url.trim().trim_end_matches('/').to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("invalid: base_url: must start with http:// or https://".into());
    }
    let name = settings.operator_name.trim().to_string();
    if name.is_empty() || name.chars().count() > 64 {
        return Err("invalid: operator_name: 1–64 characters".into());
    }
    let settings = CashSettings {
        base_url: url,
        operator_name: name,
    };
    let raw = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(data_dir(&app)?.join("crm-cash-desk.json"), raw)
        .map_err(|e| format!("save cash-desk settings: {e}"))?;
    Ok(settings)
}

/// Device status for the cash-desk header.
#[derive(Debug, Clone, Serialize)]
pub struct CashDeviceStatus {
    /// Whether the DTO10 web server answered.
    pub reachable: bool,
    /// `opened`, `closed` or `expired`.
    pub shift_state: Option<String>,
    /// Current (or last) shift number.
    pub shift_number: Option<u64>,
    /// Why the device could not be read.
    pub error: Option<String>,
}

fn device_error(state: TaskState) -> String {
    match state {
        TaskState::Failed { code, description } => format!("ККМ отказала ({code}): {description}"),
        TaskState::Pending => "ККМ не ответила вовремя".to_string(),
        TaskState::Ambiguous(status) => format!("ККМ: статус {status}"),
        TaskState::Unknown => "ККМ потеряла задание".to_string(),
        TaskState::Ready(_) => String::new(),
    }
}

/// Shift state as the device reports it.
#[tauri::command]
pub async fn cash_device_status(app: AppHandle) -> Result<CashDeviceStatus, String> {
    let settings = load_settings(&app);
    let client = client(&settings)?;
    match client.run(&Task::ShiftStatus, REPORT_TIMEOUT).await {
        Ok(TaskState::Ready(result)) => {
            let status = parse_shift_status(&result);
            Ok(CashDeviceStatus {
                reachable: true,
                shift_state: status.as_ref().map(|s| s.state.clone()),
                shift_number: status.map(|s| s.number),
                error: None,
            })
        }
        Ok(other) => Ok(CashDeviceStatus {
            reachable: true,
            shift_state: None,
            shift_number: None,
            error: Some(device_error(other)),
        }),
        Err(error) => Ok(CashDeviceStatus {
            reachable: false,
            shift_state: None,
            shift_number: None,
            error: Some(error.to_string()),
        }),
    }
}

/// Result of a shift operation.
#[derive(Debug, Clone, Serialize)]
pub struct CashShiftResult {
    /// The shift number the device reported.
    pub shift_number: u64,
    /// Fiscal document number of the report.
    pub document_number: u64,
    /// Set when the device printed the report but the relay did not record it.
    pub record_error: Option<String>,
}

async fn shift_operation(
    action: ShiftAction,
    channel_id: &str,
    app: &AppHandle,
    state: &AppState,
) -> Result<CashShiftResult, String> {
    let channel = parse_uuid(channel_id, "channel id")?;
    let settings = load_settings(app);
    let task = match action {
        ShiftAction::Open => Task::OpenShift(operator(&settings)),
        ShiftAction::Close => Task::CloseShift(operator(&settings)),
    };
    let result = match client(&settings)?
        .run(&task, REPORT_TIMEOUT)
        .await
        .map_err(|e| e.to_string())?
    {
        TaskState::Ready(result) => result,
        other => return Err(device_error(other)),
    };
    let fiscal: FiscalParams = parse_fiscal_params(&result)
        .ok_or_else(|| "ККМ вернула отчёт без фискальных реквизитов".to_string())?;
    let payload = CashShiftPayload {
        v: 1,
        action,
        fn_number: fiscal.fn_number.clone(),
        shift_number: fiscal.shift_number,
        document_number: fiscal.document_number,
        document_datetime: fiscal.document_datetime.clone(),
    };
    let record_error = match buzz_sdk_pkg::build_crm_cash_shift(channel, payload) {
        Ok(builder) => submit_event(builder, state).await.err(),
        Err(error) => Some(sdk_error(error)),
    };
    Ok(CashShiftResult {
        shift_number: fiscal.shift_number,
        document_number: fiscal.document_number,
        record_error,
    })
}

/// Open a shift on the device and record it.
#[tauri::command]
pub async fn cash_open_shift(
    channel_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CashShiftResult, String> {
    shift_operation(ShiftAction::Open, &channel_id, &app, &state).await
}

/// Close the shift (Z-report) on the device and record it.
#[tauri::command]
pub async fn cash_close_shift(
    channel_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CashShiftResult, String> {
    shift_operation(ShiftAction::Close, &channel_id, &app, &state).await
}

/// Print an X-report (no fiscal record).
#[tauri::command]
pub async fn cash_report_x(app: AppHandle) -> Result<(), String> {
    let settings = load_settings(&app);
    match client(&settings)?
        .run(&Task::ReportX(operator(&settings)), REPORT_TIMEOUT)
        .await
        .map_err(|e| e.to_string())?
    {
        TaskState::Ready(_) => Ok(()),
        other => Err(device_error(other)),
    }
}

/// `GET /crm/shifts` as returned by the relay.
#[tauri::command]
pub async fn crm_list_shifts(state: State<'_, AppState>) -> Result<Value, String> {
    get_relay_json::<Value>(&state, "/crm/shifts").await
}

/// A queued receipt as the cash desk shows it.
#[derive(Debug, Clone, Serialize)]
pub struct CashJobWire {
    pub operation: String,
    pub sale_id: String,
    pub sale_number: i64,
    pub total_minor: i64,
    pub state: JobState,
    pub error: Option<String>,
    pub updated_at: i64,
}

impl From<&Job> for CashJobWire {
    fn from(job: &Job) -> Self {
        Self {
            operation: job.operation.clone(),
            sale_id: job.sale_id.to_string(),
            sale_number: job.sale_number,
            total_minor: job.receipt.total_minor,
            state: job.state,
            error: job.error.clone(),
            updated_at: job.updated_at,
        }
    }
}

fn wire<T: serde::de::DeserializeOwned>(value: &Value, what: &str) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|e| format!("unexpected {what}: {e}"))
}

/// Build the receipt for a sale from the relay's read model.
async fn receipt_for_sale(
    state: &AppState,
    sale_id: Uuid,
    is_return: bool,
) -> Result<(ReceiptInput, i64), String> {
    let sale: Value = get_relay_json(state, &format!("/crm/sales/{sale_id}")).await?;
    let mut products = HashMap::new();
    for line in sale["lines"].as_array().ok_or("sale has no lines")? {
        let product_id = line["product_id"].as_str().ok_or("line has no product")?;
        if !products.contains_key(product_id) {
            let product: Value =
                get_relay_json(state, &format!("/crm/products/{product_id}")).await?;
            products.insert(product_id.to_string(), product);
        }
    }
    receipt_from_sale(&sale, &products, is_return)
}

/// The receipt for `sale` (`GET /crm/sales/{id}`), with each line's
/// payment object (тег 1212) taken from its product.
fn receipt_from_sale(
    sale: &Value,
    products: &HashMap<String, Value>,
    is_return: bool,
) -> Result<(ReceiptInput, i64), String> {
    let total = sale["total_minor"].as_i64().ok_or("sale has no total")?;
    let mut lines = Vec::new();
    for line in sale["lines"].as_array().ok_or("sale has no lines")? {
        let product_id = line["product_id"].as_str().ok_or("line has no product")?;
        let product = products
            .get(product_id)
            .ok_or_else(|| format!("product {product_id} was not read"))?;
        lines.push(ReceiptLine {
            name: line["product_name"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            price_minor: line["price_minor"].as_i64().unwrap_or_default(),
            qty_milli: line["qty_milli"].as_i64().unwrap_or_default(),
            amount_minor: line["amount_minor"].as_i64().unwrap_or_default(),
            vat: wire::<VatRate>(&line["vat"], "VAT")?,
            unit: wire::<Unit>(&line["unit"], "unit")?,
            payment_object: wire::<PaymentObject>(&product["payment_object"], "payment object")?,
        });
    }
    let tendered = sale["cash_tendered_minor"].as_i64();
    let mut payments = Vec::new();
    for part in sale["payments"].as_array().ok_or("sale has no payments")? {
        let method = wire::<PaymentMethod>(&part["method"], "payment method")?;
        let mut amount = part["amount_minor"].as_i64().unwrap_or_default();
        // The device prints the change from the cash handed over; a return
        // pays back exactly the cash part.
        if method == PaymentMethod::Cash && !is_return {
            amount = tendered.unwrap_or(amount).max(amount);
        }
        payments.push(ReceiptPayment {
            method,
            amount_minor: amount,
        });
    }
    let number = sale["number"].as_i64().unwrap_or_default();
    Ok((
        ReceiptInput {
            is_return,
            lines,
            payments,
            total_minor: total,
            client_contact: None,
        },
        number,
    ))
}

async fn record(state: &AppState, job: Job, fiscal: FiscalParams) -> RecordOutcome {
    let builder = match buzz_sdk_pkg::build_crm_fiscal_receipt(
        job.channel_id,
        job.sale_id,
        receipt_payload(&job, fiscal),
    ) {
        Ok(builder) => builder,
        Err(error) => return RecordOutcome::Failed(sdk_error(error)),
    };
    match submit_event(builder, state).await {
        Ok(_) => RecordOutcome::Recorded,
        Err(error) if error.contains("already recorded for the sale") => {
            RecordOutcome::AlreadyRecorded
        }
        Err(error) => RecordOutcome::Failed(error),
    }
}

async fn drive(
    app: &AppHandle,
    desk: &CashDeskState,
    state: &AppState,
    only: Option<(&str, Uuid)>,
) -> Result<Vec<CashJobWire>, String> {
    let _pass = desk.pass.lock().await;
    let outbox = outbox(app, desk)?;
    let settings = load_settings(app);
    let client = client(&settings)?;
    let operator = operator(&settings);
    for job in outbox.list().map_err(|e| e.to_string())? {
        if let Some((operation, sale)) = only {
            if job.operation != operation || job.sale_id != sale {
                continue;
            }
        }
        if !matches!(job.state, JobState::Pending | JobState::Printed) {
            continue;
        }
        process(&outbox, &client, &operator, job, |job, fiscal| {
            record(state, job, fiscal)
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    list(&outbox)
}

fn list(outbox: &Outbox) -> Result<Vec<CashJobWire>, String> {
    Ok(outbox
        .list()
        .map_err(|e| e.to_string())?
        .iter()
        .map(CashJobWire::from)
        .collect())
}

/// Print the receipt for a sale (or, with `operation = "sell_return"`, the
/// return receipt of a voided sale). Returns the job afterwards, or `None`
/// once printed and recorded.
#[tauri::command]
pub async fn cash_fiscalize_sale(
    channel_id: String,
    sale_id: String,
    operation: String,
    app: AppHandle,
    desk: State<'_, CashDeskState>,
    state: State<'_, AppState>,
) -> Result<Option<CashJobWire>, String> {
    let channel = parse_uuid(&channel_id, "channel id")?;
    let sale = parse_uuid(&sale_id, "sale id")?;
    if operation != "sell" && operation != "sell_return" {
        return Err("invalid: operation must be sell or sell_return".into());
    }
    let outbox = outbox(&app, &desk)?;
    if outbox
        .get(&operation, sale)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        let (receipt, number) = receipt_for_sale(&state, sale, operation == "sell_return").await?;
        outbox
            .enqueue(&operation, sale, channel, number, &receipt)
            .map_err(|e| e.to_string())?;
    }
    drive(&app, &desk, &state, Some((&operation, sale))).await?;
    Ok(outbox
        .get(&operation, sale)
        .map_err(|e| e.to_string())?
        .as_ref()
        .map(CashJobWire::from))
}

/// The receipt queue.
#[tauri::command]
pub fn cash_queue(
    app: AppHandle,
    desk: State<'_, CashDeskState>,
) -> Result<Vec<CashJobWire>, String> {
    list(&*outbox(&app, &desk)?)
}

/// Advance every pending or printed receipt (on app start, a timer, or the
/// "Повторить" button).
#[tauri::command]
pub async fn cash_process_queue(
    app: AppHandle,
    desk: State<'_, CashDeskState>,
    state: State<'_, AppState>,
) -> Result<Vec<CashJobWire>, String> {
    drive(&app, &desk, &state, None).await
}

/// A person's decision about a stuck receipt.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CashJobAction {
    /// Failed (nothing printed): try again with a new task.
    Retry,
    /// Needs attention: the person checked the device; ask it again.
    Resume,
    /// Do not print this receipt (it was printed by hand, or the sale is wrong).
    Discard,
}

/// Apply a person's decision to a queued receipt.
#[tauri::command]
pub async fn cash_resolve_job(
    operation: String,
    sale_id: String,
    action: CashJobAction,
    app: AppHandle,
    desk: State<'_, CashDeskState>,
    state: State<'_, AppState>,
) -> Result<Vec<CashJobWire>, String> {
    let sale = parse_uuid(&sale_id, "sale id")?;
    let outbox = outbox(&app, &desk)?;
    match action {
        CashJobAction::Retry => outbox.retry(&operation, sale),
        CashJobAction::Resume => outbox.resume(&operation, sale),
        CashJobAction::Discard => outbox.remove(&operation, sale),
    }
    .map_err(|e| e.to_string())?;
    if action == CashJobAction::Discard {
        return list(&outbox);
    }
    drive(&app, &desk, &state, Some((&operation, sale))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_point_at_the_local_dto10() {
        let settings = CashSettings::default();
        assert_eq!(settings.base_url, "http://127.0.0.1:16732");
        assert!(!settings.operator_name.is_empty());
    }

    #[test]
    fn device_errors_read_as_sentences() {
        assert_eq!(
            device_error(TaskState::Failed {
                code: 69,
                description: "Смена не открыта".into()
            }),
            "ККМ отказала (69): Смена не открыта"
        );
        assert!(device_error(TaskState::Unknown).contains("потеряла"));
    }

    fn sale_json() -> Value {
        serde_json::json!({
            "number": 7,
            "total_minor": 47_975,
            "cash_tendered_minor": 50_000,
            "payments": [
                {"method": "cash", "amount_minor": 27_975},
                {"method": "card", "amount_minor": 20_000}
            ],
            "lines": [
                {"product_id": "k", "product_name": "Кефир", "unit": "piece",
                 "vat": "vat22", "qty_milli": 2000, "price_minor": 8990,
                 "discount_minor": 0, "amount_minor": 17_980},
                {"product_id": "c", "product_name": "Сыр", "unit": "kilogram",
                 "vat": "vat10", "qty_milli": 500, "price_minor": 59_990,
                 "discount_minor": 0, "amount_minor": 29_995}
            ]
        })
    }

    fn products() -> HashMap<String, Value> {
        HashMap::from([
            (
                "k".to_string(),
                serde_json::json!({"payment_object": "commodity"}),
            ),
            (
                "c".to_string(),
                serde_json::json!({"payment_object": "commodity"}),
            ),
        ])
    }

    #[test]
    fn a_sale_receipt_prints_the_cash_handed_over() {
        let (receipt, number) = receipt_from_sale(&sale_json(), &products(), false).unwrap();
        assert_eq!(number, 7);
        assert_eq!(receipt.total_minor, 47_975);
        assert_eq!(receipt.lines.len(), 2);
        assert_eq!(receipt.lines[1].vat, VatRate::Vat10);
        let cash = &receipt.payments[0];
        assert_eq!(
            (cash.method, cash.amount_minor),
            (PaymentMethod::Cash, 50_000)
        );
        let card = &receipt.payments[1];
        assert_eq!(
            (card.method, card.amount_minor),
            (PaymentMethod::Card, 20_000)
        );
    }

    #[test]
    fn a_return_receipt_pays_back_exactly_the_cash_part() {
        let (receipt, _) = receipt_from_sale(&sale_json(), &products(), true).unwrap();
        assert!(receipt.is_return);
        assert_eq!(receipt.payments[0].amount_minor, 27_975);
    }

    #[test]
    fn a_line_without_its_product_is_an_error_not_a_guess() {
        let mut products = products();
        products.remove("c");
        assert!(receipt_from_sale(&sale_json(), &products, false).is_err());
    }
}
