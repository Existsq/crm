//! `buzz crm payroll` — staff pay rates, payouts (kinds 42160–42162) and the
//! payroll read.

use buzz_crm::{Money, PayRatesPayload, PayoutPayload};

use super::parties::with_query;
use crate::client::{create_response_with_id_if_accepted, normalize_write_response, BuzzClient};
use crate::error::CliError;
use crate::validate::{parse_uuid, sdk_err};
use crate::CrmPayrollCmd;

/// Run a `buzz crm payroll` subcommand and return the JSON it prints.
pub async fn run(cmd: CrmPayrollCmd, client: &BuzzClient) -> Result<String, CliError> {
    match cmd {
        CrmPayrollCmd::Show { from, to } => {
            let mut params = url::form_urlencoded::Serializer::new(String::new());
            for (key, value) in [("from", from), ("to", to)] {
                if let Some(day) = value {
                    check_day(&day, key)?;
                    params.append_pair(key, &day);
                }
            }
            client
                .get_authed(&with_query("/crm/payroll", params.finish()))
                .await
        }
        CrmPayrollCmd::Rates {
            channel,
            staff,
            percent,
            per_visit,
            shift,
        } => {
            let channel = parse_uuid(&channel)?;
            let payload = PayRatesPayload {
                v: buzz_crm::space::PAYLOAD_VERSION,
                staff_id: parse_uuid(&staff)?,
                percent_bp: parse_percent_bp(&percent)?,
                per_visit_minor: rubles(&per_visit, "per-visit")?,
                shift_minor: rubles(&shift, "shift")?,
            };
            let builder = buzz_sdk::build_crm_pay_rates(channel, payload).map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            Ok(normalize_write_response(&resp))
        }
        CrmPayrollCmd::Pay {
            channel,
            staff,
            amount,
            date,
            note,
        } => {
            let channel = parse_uuid(&channel)?;
            check_day(&date, "date")?;
            let payload = PayoutPayload {
                v: buzz_crm::space::PAYLOAD_VERSION,
                staff_id: parse_uuid(&staff)?,
                amount_minor: rubles(&amount, "amount")?,
                date,
                note,
            };
            let payout_id = uuid::Uuid::new_v4();
            let builder =
                buzz_sdk::build_crm_payout(channel, payout_id, payload).map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            Ok(create_response_with_id_if_accepted(
                &resp,
                "payout_id",
                &payout_id.to_string(),
            ))
        }
        CrmPayrollCmd::VoidPayout { channel, payout } => {
            let builder =
                buzz_sdk::build_crm_payout_void(parse_uuid(&channel)?, parse_uuid(&payout)?)
                    .map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            Ok(normalize_write_response(&resp))
        }
    }
}

fn check_day(day: &str, flag: &str) -> Result<(), CliError> {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| CliError::Usage(format!("--{flag} must be YYYY-MM-DD, got {day:?}")))
}

fn rubles(raw: &str, flag: &'static str) -> Result<i64, CliError> {
    Money::parse_rubles(raw, flag)
        .map(Money::minor)
        .map_err(|e| CliError::Usage(e.to_string()))
}

/// `40` or `12.5` percent as basis points (at most two decimals).
fn parse_percent_bp(raw: &str) -> Result<u32, CliError> {
    let bad = || {
        CliError::Usage(format!(
            "--percent must be 0–100 with up to 2 decimals, got {raw:?}"
        ))
    };
    let (whole, fraction) = raw.trim().split_once('.').unwrap_or((raw.trim(), ""));
    if whole.is_empty()
        || fraction.len() > 2
        || !whole.chars().all(|c| c.is_ascii_digit())
        || !fraction.chars().all(|c| c.is_ascii_digit())
    {
        return Err(bad());
    }
    let whole: u32 = whole.parse().map_err(|_| bad())?;
    let fraction: u32 = format!("{fraction:0<2}").parse().map_err(|_| bad())?;
    let bp = whole
        .checked_mul(100)
        .and_then(|w| w.checked_add(fraction))
        .ok_or_else(bad)?;
    if bp > buzz_crm::payroll::MAX_PERCENT_BP {
        return Err(bad());
    }
    Ok(bp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_parses_to_basis_points() {
        assert_eq!(parse_percent_bp("40").unwrap(), 4000);
        assert_eq!(parse_percent_bp("12.5").unwrap(), 1250);
        assert_eq!(parse_percent_bp("0.05").unwrap(), 5);
        assert_eq!(parse_percent_bp("100").unwrap(), 10_000);
        for bad in ["", "101", "1.234", "-5", "abc", ".5"] {
            assert!(parse_percent_bp(bad).is_err(), "{bad}");
        }
    }
}
