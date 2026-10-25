//! `buzz crm stock` — stock levels, the movement journal, stock documents
//! (kinds 42106, 42107) and service tech cards (kind 42108).

use buzz_crm::{
    Money, Qty, StockDocLine, StockDocPayload, StockDocType, TechCardItem, TechCardPayload,
};

use super::parties::with_query;
use super::sales::default_warehouse;
use crate::client::{create_response_with_id_if_accepted, normalize_write_response, BuzzClient};
use crate::error::CliError;
use crate::validate::{parse_uuid, sdk_err};
use crate::CrmStockCmd;

/// Run a `buzz crm stock` subcommand and return the JSON it prints.
pub async fn run(cmd: CrmStockCmd, client: &BuzzClient) -> Result<String, CliError> {
    match cmd {
        CrmStockCmd::Levels { warehouse, q, low } => {
            let mut params = url::form_urlencoded::Serializer::new(String::new());
            if let Some(warehouse) = warehouse.as_deref() {
                params.append_pair("warehouse", &parse_uuid(warehouse)?.to_string());
            }
            if let Some(q) = q.as_deref() {
                params.append_pair("q", q);
            }
            if low {
                params.append_pair("low", "true");
            }
            client
                .get_authed(&with_query("/crm/stock", params.finish()))
                .await
        }
        CrmStockCmd::Moves {
            warehouse,
            product,
            limit,
            before,
        } => {
            let mut params = url::form_urlencoded::Serializer::new(String::new());
            for (key, value) in [("warehouse", warehouse), ("product", product)] {
                if let Some(raw) = value {
                    params.append_pair(key, &parse_uuid(&raw)?.to_string());
                }
            }
            if let Some(limit) = limit {
                params.append_pair("limit", &limit.to_string());
            }
            if let Some(before) = before {
                params.append_pair("before", &before.to_string());
            }
            client
                .get_authed(&with_query("/crm/stock/moves", params.finish()))
                .await
        }
        CrmStockCmd::Docs {
            doc_type,
            limit,
            before,
        } => {
            let mut params = url::form_urlencoded::Serializer::new(String::new());
            if let Some(raw) = doc_type.as_deref() {
                params.append_pair("type", parse_doc_type(raw)?.as_str());
            }
            if let Some(limit) = limit {
                params.append_pair("limit", &limit.to_string());
            }
            if let Some(before) = before {
                params.append_pair("before", &before.to_string());
            }
            client
                .get_authed(&with_query("/crm/stock/docs", params.finish()))
                .await
        }
        CrmStockCmd::Doc { doc } => {
            let doc = parse_uuid(&doc)?;
            client.get_authed(&format!("/crm/stock/docs/{doc}")).await
        }
        CrmStockCmd::Post {
            channel,
            doc_type,
            warehouse,
            to_warehouse,
            supplier,
            reference,
            lines,
            note,
        } => {
            let channel = parse_uuid(&channel)?;
            let warehouse_id = match warehouse {
                Some(raw) => parse_uuid(&raw)?,
                None => default_warehouse(client).await?,
            };
            let lines = lines
                .iter()
                .map(|raw| parse_doc_line(raw))
                .collect::<Result<Vec<_>, _>>()?;
            let payload = StockDocPayload {
                v: buzz_crm::space::PAYLOAD_VERSION,
                doc_type: parse_doc_type(&doc_type)?,
                warehouse_id,
                to_warehouse_id: to_warehouse.as_deref().map(parse_uuid).transpose()?,
                supplier_id: supplier.as_deref().map(parse_uuid).transpose()?,
                reference,
                lines,
                note,
            };
            let doc_id = uuid::Uuid::new_v4();
            let builder =
                buzz_sdk::build_crm_stock_doc(channel, doc_id, payload).map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            Ok(create_response_with_id_if_accepted(
                &resp,
                "doc_id",
                &doc_id.to_string(),
            ))
        }
        CrmStockCmd::Void {
            channel,
            doc,
            reason,
        } => {
            let builder = buzz_sdk::build_crm_stock_doc_void(
                parse_uuid(&channel)?,
                parse_uuid(&doc)?,
                &reason,
            )
            .map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            Ok(normalize_write_response(&resp))
        }
        CrmStockCmd::TechCards => client.get_authed("/crm/tech-cards").await,
        CrmStockCmd::TechCard {
            channel,
            service,
            items,
            expected_revision,
        } => {
            let items = items
                .iter()
                .map(|raw| parse_item(raw))
                .collect::<Result<Vec<_>, _>>()?;
            let payload = TechCardPayload {
                v: buzz_crm::space::PAYLOAD_VERSION,
                items,
            };
            let builder = buzz_sdk::build_crm_tech_card(
                parse_uuid(&channel)?,
                parse_uuid(&service)?,
                payload,
                expected_revision.as_deref(),
            )
            .map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            Ok(normalize_write_response(&resp))
        }
    }
}

fn parse_doc_type(raw: &str) -> Result<StockDocType, CliError> {
    let wire = raw.replace('-', "_");
    StockDocType::ALL
        .into_iter()
        .find(|doc_type| doc_type.as_str() == wire)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "invalid --type {raw:?} (receipt, write-off, inventory or transfer)"
            ))
        })
}

/// One `--line PRODUCT_UUID:QTY[:COST]` flag, in units and rubles.
fn parse_doc_line(raw: &str) -> Result<StockDocLine, CliError> {
    let usage = |reason: &str| {
        CliError::Usage(format!(
            "invalid --line {raw:?}: {reason} (expected PRODUCT_UUID:QTY[:COST])"
        ))
    };
    let parts: Vec<&str> = raw.split(':').map(str::trim).collect();
    if !(2..=3).contains(&parts.len()) {
        return Err(usage("wrong number of fields"));
    }
    Ok(StockDocLine {
        product_id: parse_uuid(parts[0])?,
        qty_milli: Qty::parse_units(parts[1], "qty").map_err(|e| usage(&e.to_string()))?,
        cost_minor: parts
            .get(2)
            .filter(|cost| !cost.is_empty())
            .map(|cost| Money::parse_rubles(cost, "cost").map_err(|e| usage(&e.to_string())))
            .transpose()?,
    })
}

/// One `--item PRODUCT_UUID:QTY` flag (quantity per service, in units).
fn parse_item(raw: &str) -> Result<TechCardItem, CliError> {
    let usage = |reason: &str| {
        CliError::Usage(format!(
            "invalid --item {raw:?}: {reason} (expected PRODUCT_UUID:QTY)"
        ))
    };
    let (product, qty) = raw
        .split_once(':')
        .ok_or_else(|| usage("missing quantity"))?;
    Ok(TechCardItem {
        product_id: parse_uuid(product.trim())?,
        qty_milli: Qty::parse_units(qty.trim(), "qty").map_err(|e| usage(&e.to_string()))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRODUCT: &str = "11111111-1111-4111-8111-111111111111";

    #[test]
    fn doc_lines_and_items_parse_units_and_rubles() {
        let line = parse_doc_line(&format!("{PRODUCT}:10:250,50")).unwrap();
        assert_eq!(line.qty_milli.milli(), 10_000);
        assert_eq!(line.cost_minor.unwrap().minor(), 25_050);
        assert_eq!(
            parse_doc_line(&format!("{PRODUCT}:0,5"))
                .unwrap()
                .cost_minor,
            None
        );
        assert_eq!(
            parse_item(&format!("{PRODUCT}:0.06"))
                .unwrap()
                .qty_milli
                .milli(),
            60
        );
        for raw in [
            PRODUCT.to_string(),
            format!("{PRODUCT}:x"),
            format!("{PRODUCT}:1:2:3"),
        ] {
            assert!(parse_doc_line(&raw).is_err(), "{raw}");
        }
        assert!(parse_item(PRODUCT).is_err());
    }

    #[test]
    fn doc_types_accept_dashes() {
        assert_eq!(parse_doc_type("write-off").unwrap(), StockDocType::WriteOff);
        assert_eq!(
            parse_doc_type("inventory").unwrap(),
            StockDocType::Inventory
        );
        assert!(parse_doc_type("gift").is_err());
    }
}
