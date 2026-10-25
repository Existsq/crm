#![deny(unsafe_code)]
//! Emulator of the ATOL DTO10 web server (`/api/v2/requests`).
//!
//! It behaves like the driver as far as the CRM depends on it:
//!
//! - `POST /api/v2/requests` takes `{"uuid", "request": [task]}` and answers
//!   `201`; a UUID seen before answers `409` without running anything again.
//! - Tasks run asynchronously: `GET /api/v2/requests/{uuid}` reports `wait`,
//!   then `ready` (or `error`) after [`EmulatorConfig::processing_delay`].
//! - A fiscal storage keeps the shift state and the fiscal document, receipt
//!   and shift counters; `sell`/`sellReturn` need an open shift.
//!
//! Test hooks: `POST /emulator/next-outcome` (`{"outcome": "blocked"}` or
//! `{"outcome": "error", "code": 999, "description": "…"}`) makes the next
//! task end that way; `POST /emulator/forget` drops all task results (a
//! driver restart); `GET /emulator/state` shows the device.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Value};

/// Fiscal storage number reported by the emulator.
pub const FN_NUMBER: &str = "9999078900004792";
/// ККТ registration number reported by the emulator.
pub const REGISTRATION_NUMBER: &str = "0000000001002292";

/// Emulator settings.
#[derive(Debug, Clone)]
pub struct EmulatorConfig {
    /// Time a task stays in `wait` before it completes.
    pub processing_delay: Duration,
    /// Whether a shift is open at start.
    pub shift_open: bool,
}

impl Default for EmulatorConfig {
    fn default() -> Self {
        Self {
            processing_delay: Duration::from_millis(300),
            shift_open: false,
        }
    }
}

#[derive(Debug, Clone)]
enum Forced {
    Blocked,
    Error { code: i64, description: String },
}

#[derive(Debug, Default)]
struct Device {
    shift_open: bool,
    shift_number: u64,
    document_number: u64,
    receipt_number: u64,
    /// Every printed document, newest last (for `GET /emulator/state`).
    printed: Vec<Value>,
}

#[derive(Debug)]
struct Emulator {
    config: EmulatorConfig,
    device: Device,
    /// Task results by UUID (`None` while the task waits).
    tasks: HashMap<String, Option<Value>>,
    forced: Option<Forced>,
}

type Shared = Arc<Mutex<Emulator>>;

fn lock(state: &Shared) -> std::sync::MutexGuard<'_, Emulator> {
    // A panicked handler must not wedge the emulator for the other tests.
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The emulator's routes.
pub fn router(config: EmulatorConfig) -> Router {
    let shift_open = config.shift_open;
    let state: Shared = Arc::new(Mutex::new(Emulator {
        config,
        device: Device {
            shift_open,
            shift_number: u64::from(shift_open),
            document_number: u64::from(shift_open),
            ..Device::default()
        },
        tasks: HashMap::new(),
        forced: None,
    }));
    Router::new()
        .route("/api/v2/requests", post(submit))
        .route("/api/v2/requests/{uuid}", get(result))
        .route("/emulator/state", get(device_state))
        .route("/emulator/next-outcome", post(next_outcome))
        .route("/emulator/forget", post(forget))
        .with_state(state)
}

/// Serve the emulator on `addr` until the process exits.
pub async fn serve(addr: SocketAddr, config: EmulatorConfig) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(config)).await
}

/// Start the emulator on a free local port; returns its base URL.
pub async fn spawn(config: EmulatorConfig) -> std::io::Result<String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router(config)).await {
            eprintln!("ATOL emulator stopped: {error}");
        }
    });
    Ok(format!("http://{addr}"))
}

#[derive(Deserialize)]
struct Submit {
    uuid: String,
    request: Vec<Value>,
}

async fn submit(State(state): State<Shared>, Json(body): Json<Submit>) -> Response {
    let delay = {
        let mut emulator = lock(&state);
        if emulator.tasks.contains_key(&body.uuid) {
            return (
                StatusCode::CONFLICT,
                Json(json!({"error": {"code": 409, "description": "Задание с таким UUID уже существует"}})),
            )
                .into_response();
        }
        emulator.tasks.insert(body.uuid.clone(), None);
        emulator.config.processing_delay
    };
    let task_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let mut emulator = lock(&task_state);
        let outcome = run(&mut emulator, &body.request);
        emulator.tasks.insert(body.uuid, Some(outcome));
    });
    (StatusCode::CREATED, Json(json!({}))).into_response()
}

async fn result(State(state): State<Shared>, Path(uuid): Path<String>) -> Response {
    let emulator = lock(&state);
    match emulator.tasks.get(&uuid) {
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"code": 404, "description": "Задание не найдено"}})),
        )
            .into_response(),
        Some(None) => Json(json!({"results": [{"status": "wait"}]})).into_response(),
        Some(Some(outcome)) => Json(json!({"results": [outcome]})).into_response(),
    }
}

async fn device_state(State(state): State<Shared>) -> Json<Value> {
    let emulator = lock(&state);
    let device = &emulator.device;
    Json(json!({
        "shift_open": device.shift_open,
        "shift_number": device.shift_number,
        "document_number": device.document_number,
        "receipt_number": device.receipt_number,
        "printed": device.printed,
    }))
}

#[derive(Deserialize)]
struct NextOutcome {
    outcome: String,
    code: Option<i64>,
    description: Option<String>,
}

async fn next_outcome(State(state): State<Shared>, Json(body): Json<NextOutcome>) -> StatusCode {
    let forced = match body.outcome.as_str() {
        "blocked" => Forced::Blocked,
        "error" => Forced::Error {
            code: body.code.unwrap_or(999),
            description: body
                .description
                .unwrap_or_else(|| "Ошибка эмулятора".into()),
        },
        _ => return StatusCode::BAD_REQUEST,
    };
    lock(&state).forced = Some(forced);
    StatusCode::NO_CONTENT
}

async fn forget(State(state): State<Shared>) -> StatusCode {
    lock(&state).tasks.clear();
    StatusCode::NO_CONTENT
}

fn error(code: i64, description: &str) -> Value {
    json!({"status": "error", "errorCode": code, "errorDescription": description})
}

fn ready(result: Value) -> Value {
    json!({"status": "ready", "errorCode": 0, "errorDescription": "", "result": result})
}

fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    // Device clocks print local time; the emulator reports UTC.
    let days = secs / 86_400;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}+00:00")
}

/// Days since 1970-01-01 → (year, month, day) (H. Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn sign(document_number: u64) -> String {
    // Stable pseudo fiscal sign: 10 digits derived from the document number.
    format!(
        "{:010}",
        (document_number.wrapping_mul(2_654_435_761)) % 10_000_000_000
    )
}

fn run(emulator: &mut Emulator, request: &[Value]) -> Value {
    if let Some(forced) = emulator.forced.take() {
        return match forced {
            Forced::Blocked => json!({"status": "blocked"}),
            Forced::Error { code, description } => error(code, &description),
        };
    }
    let Some(task) = request.first() else {
        return error(1, "Пустое задание");
    };
    let device = &mut emulator.device;
    match task["type"].as_str().unwrap_or_default() {
        "getShiftStatus" => ready(json!({"shiftStatus": {
            "state": if device.shift_open { "opened" } else { "closed" },
            "number": device.shift_number,
            "expiredTime": ""
        }})),
        "openShift" => {
            if device.shift_open {
                return error(68, "Смена открыта – операция невозможна");
            }
            device.shift_open = true;
            device.shift_number += 1;
            device.receipt_number = 0;
            device.document_number += 1;
            device
                .printed
                .push(json!({"type": "openShift", "shift": device.shift_number}));
            ready(json!({"fiscalParams": fiscal(device, None)}))
        }
        "closeShift" => {
            if !device.shift_open {
                return error(69, "Смена не открыта – операция невозможна");
            }
            device.shift_open = false;
            device.document_number += 1;
            device
                .printed
                .push(json!({"type": "closeShift", "shift": device.shift_number}));
            ready(json!({"fiscalParams": fiscal(device, None)}))
        }
        "reportX" => {
            if !device.shift_open {
                return error(69, "Смена не открыта – операция невозможна");
            }
            device
                .printed
                .push(json!({"type": "reportX", "shift": device.shift_number}));
            ready(json!({}))
        }
        kind @ ("sell" | "sellReturn") => receipt(device, kind, task),
        other => error(2, &format!("Неизвестный тип задания: {other}")),
    }
}

fn fiscal(device: &Device, total: Option<f64>) -> Value {
    let mut params = json!({
        "fiscalDocumentNumber": device.document_number,
        "fiscalDocumentSign": sign(device.document_number),
        "fiscalDocumentDateTime": now_iso(),
        "shiftNumber": device.shift_number,
        "fnNumber": FN_NUMBER,
        "registrationNumber": REGISTRATION_NUMBER,
    });
    if let Some(total) = total {
        params["fiscalReceiptNumber"] = json!(device.receipt_number);
        params["total"] = json!(total);
    }
    params
}

/// Kopecks of a DTO10 money number (`89.9` → 8990).
fn kopecks(value: &Value) -> Option<i64> {
    value.as_f64().map(|rubles| (rubles * 100.0).round() as i64)
}

fn receipt(device: &mut Device, kind: &str, task: &Value) -> Value {
    if !device.shift_open {
        return error(69, "Смена не открыта – операция невозможна");
    }
    let Some(items) = task["items"].as_array().filter(|items| !items.is_empty()) else {
        return error(3, "В чеке нет позиций");
    };
    let mut total = 0_i64;
    for item in items {
        let (Some(price), Some(quantity), Some(amount)) = (
            kopecks(&item["price"]),
            item["quantity"].as_f64(),
            kopecks(&item["amount"]),
        ) else {
            return error(4, "Позиция без цены, количества или суммы");
        };
        let gross = (price as f64 * quantity).round() as i64;
        if amount > gross || item["name"].as_str().unwrap_or_default().is_empty() {
            return error(5, "Неверная сумма позиции");
        }
        total += amount;
    }
    let paid: i64 = task["payments"]
        .as_array()
        .map(|payments| payments.iter().filter_map(|p| kopecks(&p["sum"])).sum())
        .unwrap_or_default();
    if paid < total {
        return error(6, "Сумма оплаты меньше суммы чека");
    }
    let electronic: i64 = task["payments"]
        .as_array()
        .map(|payments| {
            payments
                .iter()
                .filter(|p| p["type"] != "cash")
                .filter_map(|p| kopecks(&p["sum"]))
                .sum()
        })
        .unwrap_or_default();
    if electronic > total {
        return error(7, "Безналичная оплата больше суммы чека");
    }
    device.receipt_number += 1;
    device.document_number += 1;
    let total_rubles = total as f64 / 100.0;
    device.printed.push(json!({
        "type": kind,
        "shift": device.shift_number,
        "document": device.document_number,
        "total": total_rubles,
        "change": (paid - total) as f64 / 100.0,
        "items": items.len(),
    }));
    ready(json!({"fiscalParams": fiscal(device, Some(total_rubles))}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_751), (2026, 10, 25));
    }
}
