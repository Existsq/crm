//! `buzz crm sales` and `buzz crm analytics` — sale documents (kinds 42104,
//! 42105) and the sales/customer analytics read.

use buzz_crm::{Money, PaymentMethod, Qty, SaleLine, SalePayload};

use super::parties::with_query;
use crate::client::{create_response_with_id_if_accepted, normalize_write_response, BuzzClient};
use crate::error::CliError;
use crate::validate::{parse_uuid, sdk_err};
use crate::CrmSalesCmd;

/// Run a `buzz crm sales` subcommand and return the JSON it prints.
pub async fn run(cmd: CrmSalesCmd, client: &BuzzClient) -> Result<String, CliError> {
    match cmd {
        CrmSalesCmd::List {
            customer,
            hide_voided,
            limit,
            before,
        } => {
            let mut params = url::form_urlencoded::Serializer::new(String::new());
            if let Some(customer) = customer.as_deref() {
                params.append_pair("customer", &parse_uuid(customer)?.to_string());
            }
            if hide_voided {
                params.append_pair("voided", "false");
            }
            if let Some(limit) = limit {
                params.append_pair("limit", &limit.to_string());
            }
            if let Some(before) = before {
                params.append_pair("before", &before.to_string());
            }
            client
                .get_authed(&with_query("/crm/sales", params.finish()))
                .await
        }
        CrmSalesCmd::Get { sale } => {
            let sale = parse_uuid(&sale)?;
            client.get_authed(&format!("/crm/sales/{sale}")).await
        }
        CrmSalesCmd::Create {
            channel,
            warehouse,
            customer,
            payment,
            lines,
            note,
        } => {
            let channel = parse_uuid(&channel)?;
            let warehouse_id = match warehouse {
                Some(raw) => parse_uuid(&raw)?,
                None => default_warehouse(client).await?,
            };
            let customer_id = customer.as_deref().map(parse_uuid).transpose()?;
            let payment: PaymentMethod =
                serde_json::from_value(serde_json::Value::String(payment.clone()))
                    .map_err(|_| CliError::Usage(format!("invalid --payment {payment:?}")))?;
            let mut sale_lines = Vec::with_capacity(lines.len());
            for raw in &lines {
                let spec = parse_line(raw)?;
                let price = match spec.price {
                    Some(price) => price,
                    None => catalog_price(client, spec.product_id).await?,
                };
                sale_lines.push(SaleLine {
                    product_id: spec.product_id,
                    qty_milli: spec.qty,
                    price_minor: price,
                    discount_minor: spec.discount,
                });
            }
            let payload = SalePayload {
                v: buzz_crm::space::PAYLOAD_VERSION,
                warehouse_id,
                customer_id,
                payment,
                payments: Vec::new(),
                cash_tendered_minor: None,
                lines: sale_lines,
                note,
                appointment_id: None,
            };
            let sale_id = uuid::Uuid::new_v4();
            let builder = buzz_sdk::build_crm_sale(channel, sale_id, payload).map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            Ok(create_response_with_id_if_accepted(
                &resp,
                "sale_id",
                &sale_id.to_string(),
            ))
        }
        CrmSalesCmd::Void {
            channel,
            sale,
            reason,
        } => {
            let channel = parse_uuid(&channel)?;
            let sale = parse_uuid(&sale)?;
            let builder = buzz_sdk::build_crm_sale_void(channel, sale, &reason).map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            Ok(normalize_write_response(&resp))
        }
    }
}

/// `buzz crm analytics` — `GET /crm/analytics` for the given days.
pub async fn analytics(
    client: &BuzzClient,
    from: Option<String>,
    to: Option<String>,
) -> Result<String, CliError> {
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in [("from", from), ("to", to)] {
        if let Some(day) = value {
            chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d")
                .map_err(|_| CliError::Usage(format!("--{key} must be YYYY-MM-DD, got {day:?}")))?;
            params.append_pair(key, &day);
        }
    }
    client
        .get_authed(&with_query("/crm/analytics", params.finish()))
        .await
}

/// One `--line PRODUCT:QTY[:PRICE[:DISCOUNT]]` flag.
#[derive(Debug, PartialEq)]
struct LineSpec {
    product_id: uuid::Uuid,
    qty: Qty,
    price: Option<Money>,
    discount: Money,
}

fn parse_line(raw: &str) -> Result<LineSpec, CliError> {
    let usage = |reason: &str| {
        CliError::Usage(format!(
            "invalid --line {raw:?}: {reason} (expected PRODUCT_UUID:QTY[:PRICE[:DISCOUNT]])"
        ))
    };
    let parts: Vec<&str> = raw.split(':').map(str::trim).collect();
    if !(2..=4).contains(&parts.len()) {
        return Err(usage("wrong number of fields"));
    }
    let product_id = parse_uuid(parts[0])?;
    let qty = Qty::parse_units(parts[1], "qty").map_err(|e| usage(&e.to_string()))?;
    let money = |part: Option<&&str>, field| -> Result<Option<Money>, CliError> {
        part.filter(|value| !value.is_empty())
            .map(|value| Money::parse_rubles(value, field).map_err(|e| usage(&e.to_string())))
            .transpose()
    };
    Ok(LineSpec {
        product_id,
        qty,
        price: money(parts.get(2), "price")?,
        discount: money(parts.get(3), "discount")?.unwrap_or(Money::ZERO),
    })
}

async fn catalog_price(client: &BuzzClient, product: uuid::Uuid) -> Result<Money, CliError> {
    let body: serde_json::Value = serde_json::from_str(
        &client
            .get_authed(&format!("/crm/products/{product}"))
            .await?,
    )
    .map_err(|e| CliError::Other(format!("unexpected /crm/products response: {e}")))?;
    body["price_minor"]
        .as_i64()
        .and_then(|minor| Money::from_minor(minor).ok())
        .ok_or_else(|| CliError::Other(format!("product {product} has no price")))
}

pub(super) async fn default_warehouse(client: &BuzzClient) -> Result<uuid::Uuid, CliError> {
    let space: serde_json::Value = serde_json::from_str(&client.get_authed("/crm/space").await?)
        .map_err(|e| CliError::Other(format!("unexpected /crm/space response: {e}")))?;
    space["warehouses"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["is_default"] == true))
        .and_then(|row| row["id"].as_str())
        .ok_or_else(|| CliError::Usage("you are not a member of the CRM space".into()))
        .and_then(parse_uuid)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRODUCT: &str = "11111111-1111-4111-8111-111111111111";

    #[test]
    fn line_flags_parse_units_and_rubles() {
        let line = parse_line(&format!("{PRODUCT}:1,5:89.90:10")).unwrap();
        assert_eq!(line.qty.milli(), 1_500);
        assert_eq!(line.price.unwrap().minor(), 8_990);
        assert_eq!(line.discount.minor(), 1_000);
        let line = parse_line(&format!("{PRODUCT}:2")).unwrap();
        assert_eq!((line.qty.milli(), line.price), (2_000, None));
        assert_eq!(line.discount, Money::ZERO);
    }

    #[test]
    fn malformed_line_flags_are_usage_errors() {
        for raw in [
            PRODUCT.to_string(),
            format!("{PRODUCT}:x"),
            format!("{PRODUCT}:1:-5"),
            format!("{PRODUCT}:1:1:1:1"),
            "not-a-uuid:1".to_string(),
        ] {
            assert!(parse_line(&raw).is_err(), "{raw}");
        }
    }
}
