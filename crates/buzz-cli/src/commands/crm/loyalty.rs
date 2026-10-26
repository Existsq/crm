//! `buzz crm loyalty` — the bonus programme (kind 42170), manual bonus
//! adjustments (kind 42171) and the loyalty read.

use buzz_crm::{BonusAdjustPayload, LoyaltySettingsPayload, Money};

use super::parties::with_query;
use super::payroll::parse_percent_bp;
use crate::client::{create_response_with_id_if_accepted, normalize_write_response, BuzzClient};
use crate::error::CliError;
use crate::validate::{parse_uuid, sdk_err};
use crate::CrmLoyaltyCmd;

/// Run a `buzz crm loyalty` subcommand and return the JSON it prints.
pub async fn run(cmd: CrmLoyaltyCmd, client: &BuzzClient) -> Result<String, CliError> {
    match cmd {
        CrmLoyaltyCmd::Show { customer } => {
            let mut params = url::form_urlencoded::Serializer::new(String::new());
            if let Some(customer) = customer.as_deref() {
                params.append_pair("customer_id", &parse_uuid(customer)?.to_string());
            }
            client
                .get_authed(&with_query("/crm/loyalty", params.finish()))
                .await
        }
        CrmLoyaltyCmd::Set {
            channel,
            enabled,
            cashback,
            max_redeem,
        } => {
            let payload = LoyaltySettingsPayload {
                v: buzz_crm::space::PAYLOAD_VERSION,
                enabled,
                cashback_bp: parse_percent_bp(&cashback)?,
                max_redeem_bp: parse_percent_bp(&max_redeem)?,
            };
            let builder =
                buzz_sdk::build_crm_loyalty(parse_uuid(&channel)?, payload).map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            Ok(normalize_write_response(&resp))
        }
        CrmLoyaltyCmd::Adjust {
            channel,
            customer,
            amount,
            reason,
        } => {
            let payload = BonusAdjustPayload {
                v: buzz_crm::space::PAYLOAD_VERSION,
                customer_id: parse_uuid(&customer)?,
                delta_minor: signed_rubles(&amount)?,
                reason,
            };
            let adjustment_id = uuid::Uuid::new_v4();
            let builder =
                buzz_sdk::build_crm_bonus_adjust(parse_uuid(&channel)?, adjustment_id, payload)
                    .map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            Ok(create_response_with_id_if_accepted(
                &resp,
                "adjustment_id",
                &adjustment_id.to_string(),
            ))
        }
    }
}

/// `500`, `-120.50` rubles as signed kopecks.
fn signed_rubles(raw: &str) -> Result<i64, CliError> {
    let trimmed = raw.trim();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    let minor = Money::parse_rubles(digits, "amount")
        .map_err(|e| CliError::Usage(e.to_string()))?
        .minor();
    Ok(if negative { -minor } else { minor })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_amounts_parse_to_kopecks() {
        assert_eq!(signed_rubles("500").unwrap(), 50_000);
        assert_eq!(signed_rubles("-120.50").unwrap(), -12_050);
        assert!(signed_rubles("--1").is_err());
        assert!(signed_rubles("abc").is_err());
    }
}
