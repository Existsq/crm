//! The receipt driver against the DTO10 emulator.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use buzz_atol::wire::task_uuid;
use buzz_atol::{
    process, AtolClient, JobState, Operator, Outbox, ReceiptInput, ReceiptLine, ReceiptPayment,
    RecordOutcome, Task, TaskState,
};
use buzz_atol_emulator::{spawn, EmulatorConfig, FN_NUMBER};
use buzz_crm::{PaymentMethod, PaymentObject, Unit, VatRate};
use serde_json::{json, Value};
use uuid::Uuid;

fn operator() -> Operator {
    Operator {
        name: "Кассир".into(),
        vatin: None,
    }
}

fn receipt(total: i64, cash: i64) -> ReceiptInput {
    ReceiptInput {
        is_return: false,
        lines: vec![ReceiptLine {
            name: "Кефир".into(),
            price_minor: total,
            qty_milli: 1000,
            amount_minor: total,
            vat: VatRate::Vat10,
            unit: Unit::Piece,
            payment_object: PaymentObject::Commodity,
        }],
        payments: vec![ReceiptPayment {
            method: PaymentMethod::Cash,
            amount_minor: cash,
        }],
        total_minor: total,
        client_contact: None,
    }
}

async fn setup(shift_open: bool) -> (AtolClient, String) {
    let url = spawn(EmulatorConfig {
        processing_delay: Duration::from_millis(30),
        shift_open,
    })
    .await
    .unwrap();
    let mut client = AtolClient::new(&url).unwrap();
    client.poll_interval = Duration::from_millis(10);
    (client, url)
}

async fn emulator(url: &str, path: &str, body: Option<Value>) -> Value {
    let http = reqwest::Client::new();
    let response = match body {
        Some(body) => http.post(format!("{url}{path}")).json(&body).send().await,
        None => http.get(format!("{url}{path}")).send().await,
    }
    .unwrap();
    response.json().await.unwrap_or(Value::Null)
}

type Recorded = Arc<Mutex<Vec<(String, u64)>>>;

fn recorder(
    recorded: &Recorded,
    outcome: RecordOutcome,
) -> impl FnOnce(buzz_atol::Job, buzz_crm::FiscalParams) -> std::future::Ready<RecordOutcome> {
    let recorded = recorded.clone();
    move |job, fiscal| {
        recorded
            .lock()
            .unwrap()
            .push((job.operation.clone(), fiscal.document_number));
        std::future::ready(outcome)
    }
}

#[tokio::test]
async fn a_sale_prints_once_records_and_leaves_the_queue() {
    let (client, url) = setup(false).await;
    let open = client
        .run(&Task::OpenShift(operator()), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(matches!(open, TaskState::Ready(_)));

    let outbox = Outbox::in_memory().unwrap();
    let sale = Uuid::new_v4();
    let job = outbox
        .enqueue("sell", sale, Uuid::new_v4(), 1, &receipt(8_990, 10_000))
        .unwrap();
    // A double click queues nothing new.
    outbox
        .enqueue("sell", sale, Uuid::new_v4(), 1, &receipt(8_990, 10_000))
        .unwrap();
    assert_eq!(outbox.list().unwrap().len(), 1);

    let recorded: Recorded = Arc::default();
    let state = process(
        &outbox,
        &client,
        &operator(),
        job,
        recorder(&recorded, RecordOutcome::Recorded),
    )
    .await
    .unwrap();
    assert_eq!(state, None);
    assert!(outbox.list().unwrap().is_empty());
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[("sell".to_string(), 2)]
    );

    let device = emulator(&url, "/emulator/state", None).await;
    let printed = device["printed"].as_array().unwrap();
    assert_eq!(printed.len(), 2, "open shift + one receipt: {device}");
    assert_eq!(printed[1]["change"], json!(10.1));
}

#[tokio::test]
async fn a_relay_failure_keeps_the_printed_job_and_never_reprints() {
    let (client, url) = setup(true).await;
    let outbox = Outbox::in_memory().unwrap();
    let sale = Uuid::new_v4();
    let job = outbox
        .enqueue("sell", sale, Uuid::new_v4(), 7, &receipt(5_000, 5_000))
        .unwrap();
    let recorded: Recorded = Arc::default();
    let state = process(
        &outbox,
        &client,
        &operator(),
        job,
        recorder(&recorded, RecordOutcome::Failed("relay unreachable".into())),
    )
    .await
    .unwrap();
    assert_eq!(state, Some(JobState::Printed));
    let job = outbox.get("sell", sale).unwrap().unwrap();
    assert_eq!(job.error.as_deref(), Some("relay unreachable"));
    let fiscal = job.result.clone().unwrap();
    assert_eq!(fiscal["fiscalParams"]["fnNumber"], FN_NUMBER);

    // The next pass only records: the device prints nothing more.
    let state = process(
        &outbox,
        &client,
        &operator(),
        job,
        recorder(&recorded, RecordOutcome::AlreadyRecorded),
    )
    .await
    .unwrap();
    assert_eq!(state, None);
    let printed = emulator(&url, "/emulator/state", None).await["printed"].clone();
    assert_eq!(printed.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_crash_after_submit_resumes_the_same_task() {
    let (client, url) = setup(true).await;
    let outbox = Outbox::in_memory().unwrap();
    let sale = Uuid::new_v4();
    let job = outbox
        .enqueue("sell", sale, Uuid::new_v4(), 1, &receipt(1_000, 1_000))
        .unwrap();
    // The app sent the task, then died before saving the result.
    let task = Task::Receipt(operator(), job.receipt.clone());
    assert!(client
        .submit(task_uuid("sell", sale, 0), &task)
        .await
        .unwrap());
    let recorded: Recorded = Arc::default();
    let state = process(
        &outbox,
        &client,
        &operator(),
        job,
        recorder(&recorded, RecordOutcome::Recorded),
    )
    .await
    .unwrap();
    assert_eq!(state, None);
    let printed = emulator(&url, "/emulator/state", None).await["printed"].clone();
    assert_eq!(
        printed.as_array().unwrap().len(),
        1,
        "409 resumed, not reprinted"
    );
}

#[tokio::test]
async fn refusals_fail_and_retry_with_a_new_uuid() {
    let (client, _) = setup(false).await;
    let outbox = Outbox::in_memory().unwrap();
    let sale = Uuid::new_v4();
    let job = outbox
        .enqueue("sell", sale, Uuid::new_v4(), 1, &receipt(1_000, 1_000))
        .unwrap();
    let recorded: Recorded = Arc::default();
    // Shift closed: the device refuses, nothing is printed.
    let state = process(
        &outbox,
        &client,
        &operator(),
        job,
        recorder(&recorded, RecordOutcome::Recorded),
    )
    .await
    .unwrap();
    assert_eq!(state, Some(JobState::Failed));
    let job = outbox.get("sell", sale).unwrap().unwrap();
    assert!(
        job.error.as_deref().unwrap().contains("69"),
        "{:?}",
        job.error
    );

    client
        .run(&Task::OpenShift(operator()), Duration::from_secs(5))
        .await
        .unwrap();
    outbox.retry("sell", sale).unwrap();
    let job = outbox.get("sell", sale).unwrap().unwrap();
    assert_eq!((job.state, job.attempt), (JobState::Pending, 1));
    let state = process(
        &outbox,
        &client,
        &operator(),
        job,
        recorder(&recorded, RecordOutcome::Recorded),
    )
    .await
    .unwrap();
    assert_eq!(state, None);
}

#[tokio::test]
async fn ambiguous_results_need_a_person() {
    let (client, url) = setup(true).await;
    let outbox = Outbox::in_memory().unwrap();
    let recorded: Recorded = Arc::default();

    emulator(
        &url,
        "/emulator/next-outcome",
        Some(json!({"outcome": "blocked"})),
    )
    .await;
    let blocked = Uuid::new_v4();
    let job = outbox
        .enqueue("sell", blocked, Uuid::new_v4(), 1, &receipt(1_000, 1_000))
        .unwrap();
    let state = process(
        &outbox,
        &client,
        &operator(),
        job,
        recorder(&recorded, RecordOutcome::Recorded),
    )
    .await
    .unwrap();
    assert_eq!(state, Some(JobState::NeedsAttention));
    // Still needs a person on the next pass: the driver does not retry it.
    let job = outbox.get("sell", blocked).unwrap().unwrap();
    let state = process(
        &outbox,
        &client,
        &operator(),
        job,
        recorder(&recorded, RecordOutcome::Recorded),
    )
    .await
    .unwrap();
    assert_eq!(state, Some(JobState::NeedsAttention));
    assert!(recorded.lock().unwrap().is_empty());
}
