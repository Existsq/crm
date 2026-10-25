//! HTTP client for the DTO10 web server with status polling.

use std::time::Duration;

use serde_json::{json, Value};
use uuid::Uuid;

use crate::wire::Task;

/// Default DTO10 web-server address.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:16732";

/// Transport-level failure: the request may or may not have reached the
/// device, so the caller must not assume the task did not run.
#[derive(Debug, thiserror::Error)]
pub enum AtolError {
    /// The web server could not be reached or the response was cut off.
    #[error("ККМ недоступна: {0}")]
    Unreachable(String),
    /// The web server answered something this client does not understand.
    #[error("непонятный ответ ККМ: {0}")]
    Protocol(String),
}

/// Where a submitted task stands.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskState {
    /// Queued or running (`wait`, `inProgress`).
    Pending,
    /// Done; the task's `result` object.
    Ready(Value),
    /// The device refused the task; nothing was printed.
    Failed {
        /// DTO10 error code.
        code: i64,
        /// DTO10 error text.
        description: String,
    },
    /// `blocked`, `interrupted` or another state where it is unknown whether
    /// the document was printed: a person must check the device.
    Ambiguous(String),
    /// The web server does not know the UUID (driver restarted, results
    /// dropped): whether it printed is unknown.
    Unknown,
}

/// Client of one DTO10 web server.
#[derive(Debug, Clone)]
pub struct AtolClient {
    base_url: String,
    http: reqwest::Client,
    /// Delay between status polls.
    pub poll_interval: Duration,
}

impl AtolClient {
    /// A client for `base_url` (e.g. [`DEFAULT_BASE_URL`]).
    pub fn new(base_url: &str) -> Result<Self, AtolError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AtolError::Protocol(e.to_string()))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            http,
            poll_interval: Duration::from_millis(250),
        })
    }

    /// Submit `task` as `uuid`. Returns `false` when the UUID was already
    /// known to the server (HTTP 409): the earlier submission is the task.
    pub async fn submit(&self, uuid: Uuid, task: &Task) -> Result<bool, AtolError> {
        let body = json!({"uuid": uuid.to_string(), "request": [task.to_json()]});
        let response = self
            .http
            .post(format!("{}/api/v2/requests", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| AtolError::Unreachable(e.to_string()))?;
        match response.status().as_u16() {
            200 | 201 => Ok(true),
            409 => Ok(false),
            status => Err(AtolError::Protocol(format!(
                "POST /api/v2/requests answered {status}"
            ))),
        }
    }

    /// The task's current state.
    pub async fn state(&self, uuid: Uuid) -> Result<TaskState, AtolError> {
        let response = self
            .http
            .get(format!("{}/api/v2/requests/{uuid}", self.base_url))
            .send()
            .await
            .map_err(|e| AtolError::Unreachable(e.to_string()))?;
        if response.status().as_u16() == 404 {
            return Ok(TaskState::Unknown);
        }
        if !response.status().is_success() {
            return Err(AtolError::Protocol(format!(
                "GET /api/v2/requests answered {}",
                response.status()
            )));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|e| AtolError::Unreachable(e.to_string()))?;
        Ok(parse_state(&body))
    }

    /// Poll until the task leaves `Pending` or `timeout` passes (then
    /// `Pending` is returned and the task may still complete later).
    pub async fn wait(&self, uuid: Uuid, timeout: Duration) -> Result<TaskState, AtolError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let state = self.state(uuid).await?;
            if state != TaskState::Pending || tokio::time::Instant::now() >= deadline {
                return Ok(state);
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    /// Submit and wait: for reports and status queries, which are safe to
    /// re-run with a fresh UUID.
    pub async fn run(&self, task: &Task, timeout: Duration) -> Result<TaskState, AtolError> {
        let uuid = Uuid::new_v4();
        self.submit(uuid, task).await?;
        self.wait(uuid, timeout).await
    }
}

/// Interpret a `GET /api/v2/requests/{uuid}` body (the first result).
pub fn parse_state(body: &Value) -> TaskState {
    let result = &body["results"][0];
    match result["status"].as_str() {
        Some("wait" | "inProgress") => TaskState::Pending,
        Some("ready") => TaskState::Ready(result["result"].clone()),
        Some("error") => TaskState::Failed {
            code: result["errorCode"].as_i64().unwrap_or(-1),
            description: result["errorDescription"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        },
        Some(other) => TaskState::Ambiguous(other.to_owned()),
        None => TaskState::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_parse() {
        assert_eq!(
            parse_state(&json!({"results": [{"status": "inProgress"}]})),
            TaskState::Pending
        );
        assert_eq!(
            parse_state(
                &json!({"results": [{"status": "error", "errorCode": 69, "errorDescription": "Смена закрыта"}]})
            ),
            TaskState::Failed {
                code: 69,
                description: "Смена закрыта".into()
            }
        );
        assert_eq!(
            parse_state(&json!({"results": [{"status": "blocked"}]})),
            TaskState::Ambiguous("blocked".into())
        );
        assert_eq!(parse_state(&json!({})), TaskState::Unknown);
    }
}
