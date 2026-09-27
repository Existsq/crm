//! `buzz crm products` — the product catalog (kind 42102).

use buzz_crm::{Money, PaymentObject, ProductPayload, Qty, Unit, VatRate};
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::client::{create_response_with_id_if_accepted, normalize_write_response, BuzzClient};
use crate::error::CliError;
use crate::validate::{parse_uuid, sdk_err};
use crate::{CrmProductFields, CrmProductsCmd};

/// Run a `buzz crm products` subcommand and return the JSON it prints.
pub async fn run(cmd: CrmProductsCmd, client: &BuzzClient) -> Result<String, CliError> {
    match cmd {
        CrmProductsCmd::List {
            query,
            barcode,
            archived,
            limit,
            after,
        } => {
            let path = list_path(
                query.as_deref(),
                barcode.as_deref(),
                archived,
                limit,
                after.as_deref(),
            );
            client.get_authed(&path).await
        }
        CrmProductsCmd::Get { product } => {
            let product = parse_uuid(&product)?;
            client.get_authed(&format!("/crm/products/{product}")).await
        }
        CrmProductsCmd::Create { channel, fields } => {
            let channel = parse_uuid(&channel)?;
            let default_vat = match fields.vat {
                Some(_) => None,
                None => Some(space_default_vat(client).await?),
            };
            let payload = new_product(fields, default_vat)?;
            let product_id = uuid::Uuid::new_v4();
            let builder =
                buzz_sdk::build_crm_product(channel, product_id, payload, None).map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            Ok(create_response_with_id_if_accepted(
                &resp,
                "product_id",
                &product_id.to_string(),
            ))
        }
        CrmProductsCmd::Update {
            channel,
            product,
            revision,
            fields,
        } => {
            let channel = parse_uuid(&channel)?;
            let product = parse_uuid(&product)?;
            let current = current_product(client, product).await?;
            let revision = revision.unwrap_or_else(|| current.revision.clone());
            let payload = merge_product(current, fields, None)?;
            submit_update(client, channel, product, payload, &revision).await
        }
        CrmProductsCmd::Archive {
            channel,
            product,
            restore,
        } => {
            let channel = parse_uuid(&channel)?;
            let product = parse_uuid(&product)?;
            let current = current_product(client, product).await?;
            let revision = current.revision.clone();
            let payload = merge_product(current, CrmProductFields::default(), Some(!restore))?;
            submit_update(client, channel, product, payload, &revision).await
        }
    }
}

async fn submit_update(
    client: &BuzzClient,
    channel: uuid::Uuid,
    product: uuid::Uuid,
    payload: ProductPayload,
    revision: &str,
) -> Result<String, CliError> {
    let builder =
        buzz_sdk::build_crm_product(channel, product, payload, Some(revision)).map_err(sdk_err)?;
    let resp = client.submit_event(client.sign_event(builder)?).await?;
    Ok(normalize_write_response(&resp))
}

fn list_path(
    query: Option<&str>,
    barcode: Option<&str>,
    archived: bool,
    limit: Option<u32>,
    after: Option<&str>,
) -> String {
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    if let Some(q) = query {
        params.append_pair("q", q);
    }
    if let Some(barcode) = barcode {
        params.append_pair("barcode", barcode);
    }
    if archived {
        params.append_pair("archived", "true");
    }
    if let Some(limit) = limit {
        params.append_pair("limit", &limit.to_string());
    }
    if let Some(after) = after {
        params.append_pair("after", after);
    }
    let params = params.finish();
    if params.is_empty() {
        "/crm/products".to_string()
    } else {
        format!("/crm/products?{params}")
    }
}

/// The product as `GET /crm/products/{id}` returns it.
#[derive(Debug, Deserialize)]
struct CurrentProduct {
    name: String,
    sku: Option<String>,
    barcodes: Vec<String>,
    unit: Unit,
    price_minor: Money,
    vat: VatRate,
    payment_object: PaymentObject,
    category: Option<String>,
    min_stock_milli: Qty,
    requires_marking: bool,
    archived: bool,
    revision: String,
}

async fn current_product(
    client: &BuzzClient,
    product: uuid::Uuid,
) -> Result<CurrentProduct, CliError> {
    let body = client
        .get_authed(&format!("/crm/products/{product}"))
        .await?;
    serde_json::from_str(&body)
        .map_err(|e| CliError::Other(format!("unexpected /crm/products response: {e}")))
}

async fn space_default_vat(client: &BuzzClient) -> Result<VatRate, CliError> {
    let space: serde_json::Value = serde_json::from_str(&client.get_authed("/crm/space").await?)
        .map_err(|e| CliError::Other(format!("unexpected /crm/space response: {e}")))?;
    space["settings"]["default_vat"]
        .as_str()
        .ok_or_else(|| CliError::Usage("you are not a member of the CRM space".into()))?
        .parse::<VatRate>()
        .map_err(|e| CliError::Other(format!("CRM space default VAT: {e}")))
}

/// Parse a wire-name flag (`--unit piece`) into its domain enum.
fn wire<T: DeserializeOwned>(raw: &str, flag: &str) -> Result<T, CliError> {
    serde_json::from_value(serde_json::Value::String(raw.to_owned()))
        .map_err(|_| CliError::Usage(format!("invalid --{flag} {raw:?}")))
}

fn usage(error: buzz_crm::CrmError) -> CliError {
    CliError::Usage(error.to_string())
}

/// `""` clears an optional text field.
fn optional_text(raw: String) -> Option<String> {
    (!raw.trim().is_empty()).then_some(raw)
}

fn new_product(
    fields: CrmProductFields,
    default_vat: Option<VatRate>,
) -> Result<ProductPayload, CliError> {
    let name = fields
        .name
        .clone()
        .ok_or_else(|| CliError::Usage("--name is required".into()))?;
    let price = fields
        .price
        .clone()
        .ok_or_else(|| CliError::Usage("--price is required".into()))?;
    let base = CurrentProduct {
        name,
        sku: None,
        barcodes: Vec::new(),
        unit: Unit::Piece,
        price_minor: Money::parse_rubles(&price, "price").map_err(usage)?,
        vat: default_vat.unwrap_or(VatRate::Vat22),
        payment_object: PaymentObject::Commodity,
        category: None,
        min_stock_milli: Qty::ZERO,
        requires_marking: false,
        archived: false,
        revision: String::new(),
    };
    merge_product(base, fields, None)
}

/// Overlay the given flags on `current`; `archived` overrides the flag.
fn merge_product(
    current: CurrentProduct,
    fields: CrmProductFields,
    archived: Option<bool>,
) -> Result<ProductPayload, CliError> {
    let barcodes = if fields.clear_barcodes {
        Vec::new()
    } else if fields.barcodes.is_empty() {
        current.barcodes
    } else {
        fields.barcodes
    };
    Ok(ProductPayload {
        v: buzz_crm::space::PAYLOAD_VERSION,
        name: fields.name.unwrap_or(current.name),
        sku: match fields.sku {
            Some(raw) => optional_text(raw),
            None => current.sku,
        },
        barcodes,
        unit: match fields.unit {
            Some(raw) => wire(&raw, "unit")?,
            None => current.unit,
        },
        price_minor: match fields.price {
            Some(raw) => Money::parse_rubles(&raw, "price").map_err(usage)?,
            None => current.price_minor,
        },
        vat: match fields.vat {
            Some(raw) => raw
                .parse::<VatRate>()
                .map_err(|e| CliError::Usage(e.to_string()))?,
            None => current.vat,
        },
        payment_object: match fields.payment_object {
            Some(raw) => wire(&raw, "payment-object")?,
            None => current.payment_object,
        },
        category: match fields.category {
            Some(raw) => optional_text(raw),
            None => current.category,
        },
        min_stock_milli: match fields.min_stock {
            Some(raw) => Qty::parse_units(&raw, "min_stock").map_err(usage)?,
            None => current.min_stock_milli,
        },
        requires_marking: fields.requires_marking.unwrap_or(current.requires_marking),
        archived: archived.unwrap_or(current.archived),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current() -> CurrentProduct {
        serde_json::from_value(serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000000",
            "name": "Молоко",
            "sku": "MLK-1",
            "barcodes": ["4006381333931"],
            "unit": "piece",
            "price_minor": 8990,
            "vat": "vat10",
            "payment_object": "commodity",
            "category": "Молочка",
            "min_stock_milli": 5000,
            "requires_marking": false,
            "archived": false,
            "revision": "ab",
            "updated_at": "2026-09-27T00:00:00Z"
        }))
        .unwrap()
    }

    #[test]
    fn update_keeps_unspecified_fields() {
        let payload = merge_product(
            current(),
            CrmProductFields {
                price: Some("99,90".into()),
                ..CrmProductFields::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(payload.price_minor.minor(), 9990);
        assert_eq!(payload.name, "Молоко");
        assert_eq!(payload.sku.as_deref(), Some("MLK-1"));
        assert_eq!(payload.barcodes, vec!["4006381333931"]);
        assert_eq!(payload.vat, VatRate::Vat10);
        assert_eq!(payload.min_stock_milli.milli(), 5000);
        assert!(ProductPayload::parse(&serde_json::to_string(&payload).unwrap()).is_ok());
    }

    #[test]
    fn empty_text_clears_and_barcodes_are_replaced_or_cleared() {
        let payload = merge_product(
            current(),
            CrmProductFields {
                sku: Some(String::new()),
                category: Some("  ".into()),
                barcodes: vec!["INT-1".into()],
                ..CrmProductFields::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(payload.sku, None);
        assert_eq!(payload.category, None);
        assert_eq!(payload.barcodes, vec!["INT-1"]);
        let cleared = merge_product(
            current(),
            CrmProductFields {
                clear_barcodes: true,
                ..CrmProductFields::default()
            },
            Some(true),
        )
        .unwrap();
        assert!(cleared.barcodes.is_empty());
        assert!(cleared.archived);
    }

    #[test]
    fn create_requires_name_and_price_and_applies_defaults() {
        assert!(matches!(
            new_product(CrmProductFields::default(), None),
            Err(CliError::Usage(_))
        ));
        let payload = new_product(
            CrmProductFields {
                name: Some("Хлеб".into()),
                price: Some("45".into()),
                unit: Some("kilogram".into()),
                min_stock: Some("1,5".into()),
                ..CrmProductFields::default()
            },
            Some(VatRate::Vat10),
        )
        .unwrap();
        assert_eq!(payload.price_minor.minor(), 4500);
        assert_eq!(payload.unit, Unit::Kilogram);
        assert_eq!(payload.vat, VatRate::Vat10);
        assert_eq!(payload.payment_object, PaymentObject::Commodity);
        assert_eq!(payload.min_stock_milli.milli(), 1500);
    }

    #[test]
    fn bad_flags_are_usage_errors() {
        for fields in [
            CrmProductFields {
                unit: Some("bucket".into()),
                ..CrmProductFields::default()
            },
            CrmProductFields {
                price: Some("-1".into()),
                ..CrmProductFields::default()
            },
            CrmProductFields {
                payment_object: Some("gift".into()),
                ..CrmProductFields::default()
            },
            CrmProductFields {
                vat: Some("vat99".into()),
                ..CrmProductFields::default()
            },
        ] {
            assert!(matches!(
                merge_product(current(), fields, None),
                Err(CliError::Usage(_))
            ));
        }
    }

    #[test]
    fn list_path_encodes_filters() {
        assert_eq!(list_path(None, None, false, None, None), "/crm/products");
        assert_eq!(
            list_path(Some("чай 50%"), Some("INT-1"), true, Some(10), Some("c")),
            "/crm/products?q=%D1%87%D0%B0%D0%B9+50%25&barcode=INT-1&archived=true&limit=10&after=c"
        );
    }
}
