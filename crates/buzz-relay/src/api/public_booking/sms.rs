//! Delivery of booking phone codes.
//!
//! `BUZZ_BOOKING_SMS=log` (the default) writes the code to the relay log for
//! local development; `smsru` sends it through SMS.ru with
//! `BUZZ_SMS_RU_API_ID`. `BUZZ_BOOKING_EXPOSE_CODES=true` also returns the
//! code in the API response — for tests only, never in production.

use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;

use crate::config::ConfigError;

/// SMS.ru send endpoint.
const SMS_RU_URL: &str = "https://sms.ru/sms/send";
/// How long one SMS request may take.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// Where booking codes go.
#[derive(Clone, PartialEq, Eq)]
pub enum SmsProvider {
    /// Log the code (development).
    Log,
    /// Send through SMS.ru.
    SmsRu {
        /// SMS.ru API key.
        api_id: String,
    },
}

impl std::fmt::Debug for SmsProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SmsProvider::Log => f.write_str("Log"),
            SmsProvider::SmsRu { .. } => f.write_str("SmsRu { api_id: <redacted> }"),
        }
    }
}

/// Public booking configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookingConfig {
    /// Where codes go.
    pub sms: SmsProvider,
    /// Return codes in API responses (tests only).
    pub expose_codes: bool,
    /// Key rate limits by the last `X-Forwarded-For` hop (set when the relay
    /// sits behind exactly one trusted proxy).
    pub trust_forwarded_for: bool,
}

impl BookingConfig {
    /// Read `BUZZ_BOOKING_SMS`, `BUZZ_SMS_RU_API_ID`,
    /// `BUZZ_BOOKING_EXPOSE_CODES` and `BUZZ_BOOKING_TRUST_FORWARDED_FOR`.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let flag = |name: &str| {
            lookup(name)
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false)
        };
        let sms = match lookup("BUZZ_BOOKING_SMS").as_deref().unwrap_or("log") {
            "log" => SmsProvider::Log,
            "smsru" => SmsProvider::SmsRu {
                api_id: lookup("BUZZ_SMS_RU_API_ID")
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| {
                        ConfigError::InvalidValue(
                            "BUZZ_BOOKING_SMS=smsru needs BUZZ_SMS_RU_API_ID".into(),
                        )
                    })?,
            },
            other => {
                return Err(ConfigError::InvalidValue(format!(
                    "BUZZ_BOOKING_SMS must be log or smsru, not {other:?}"
                )))
            }
        };
        Ok(Self {
            sms,
            expose_codes: flag("BUZZ_BOOKING_EXPOSE_CODES"),
            trust_forwarded_for: flag("BUZZ_BOOKING_TRUST_FORWARDED_FOR"),
        })
    }
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(SEND_TIMEOUT)
            .build()
            .unwrap_or_default()
    })
}

/// Send `text` to the phone with digits `phone_key`.
pub(crate) async fn send(
    provider: &SmsProvider,
    phone_key: &str,
    text: &str,
) -> Result<(), String> {
    match provider {
        SmsProvider::Log => {
            tracing::info!(phone = %phone_key, %text, "booking SMS (BUZZ_BOOKING_SMS=log)");
            Ok(())
        }
        SmsProvider::SmsRu { api_id } => {
            let form = serde_urlencoded::to_string([
                ("api_id", api_id.as_str()),
                ("to", phone_key),
                ("msg", text),
                ("json", "1"),
            ])
            .map_err(|e| format!("SMS.ru form: {e}"))?;
            let response = client()
                .post(SMS_RU_URL)
                .header(
                    reqwest::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(form)
                .send()
                .await
                .map_err(|e| format!("SMS.ru request: {e}"))?;
            let body: Value = response
                .json()
                .await
                .map_err(|e| format!("SMS.ru response: {e}"))?;
            sms_ru_outcome(&body, phone_key)
        }
    }
}

/// SMS.ru answers `{"status": "OK", "sms": {"<phone>": {"status": "OK"}}}`.
fn sms_ru_outcome(body: &Value, phone_key: &str) -> Result<(), String> {
    let sent = body["status"] == "OK" && body["sms"][phone_key]["status"] == "OK";
    if sent {
        Ok(())
    } else {
        let reason = body["sms"][phone_key]["status_text"]
            .as_str()
            .or_else(|| body["status_text"].as_str())
            .unwrap_or("unknown error");
        Err(format!("SMS.ru refused: {reason}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(vars: &[(&str, &str)]) -> Result<BookingConfig, ConfigError> {
        BookingConfig::from_lookup(|name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        })
    }

    #[test]
    fn defaults_log_codes_and_hide_them() {
        let config = config(&[]).unwrap();
        assert_eq!(config.sms, SmsProvider::Log);
        assert!(!config.expose_codes);
        assert!(!config.trust_forwarded_for);
    }

    #[test]
    fn sms_ru_needs_a_key_and_hides_it() {
        assert!(config(&[("BUZZ_BOOKING_SMS", "smsru")]).is_err());
        assert!(config(&[("BUZZ_BOOKING_SMS", "pigeon")]).is_err());
        let config = config(&[
            ("BUZZ_BOOKING_SMS", "smsru"),
            ("BUZZ_SMS_RU_API_ID", "secret-key"),
        ])
        .unwrap();
        assert!(!format!("{config:?}").contains("secret-key"));
    }

    #[test]
    fn sms_ru_answers_are_read_per_number() {
        let ok = json!({"status": "OK", "sms": {"79000000001": {"status": "OK"}}});
        assert!(sms_ru_outcome(&ok, "79000000001").is_ok());
        let refused = json!({"status": "OK", "sms": {"79000000001": {"status": "ERROR", "status_text": "bad number"}}});
        assert_eq!(
            sms_ru_outcome(&refused, "79000000001").unwrap_err(),
            "SMS.ru refused: bad number"
        );
        assert!(sms_ru_outcome(&json!({"status": "ERROR"}), "79000000001").is_err());
    }
}
