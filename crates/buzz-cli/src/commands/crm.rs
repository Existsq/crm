//! `buzz crm` — retail CRM space and warehouses (kinds 42100–42199).

use buzz_crm::{Currency, FiscalizationMode, SpaceSettings, VatRate, WarehouseSeed};

use crate::client::{print_create_response, BuzzClient};
use crate::error::CliError;
use crate::validate::{parse_uuid, sdk_err};
use crate::{CrmCmd, CrmSpaceCmd, CrmWarehousesCmd};

/// Dispatch a `buzz crm` subcommand.
pub async fn dispatch(cmd: CrmCmd, client: &BuzzClient) -> Result<(), CliError> {
    match cmd {
        CrmCmd::Space(CrmSpaceCmd::Show) => {
            println!("{}", client.get_authed("/crm/space").await?);
            Ok(())
        }
        CrmCmd::Space(CrmSpaceCmd::Init {
            channel,
            timezone,
            fiscalization,
            default_vat,
            allow_negative_stock,
            warehouse_name,
        }) => {
            let channel = parse_uuid(&channel)?;
            let settings = SpaceSettings {
                currency: Currency::Rub,
                timezone,
                allow_negative_stock,
                fiscalization: parse_fiscalization(&fiscalization)?,
                default_vat: default_vat
                    .parse::<VatRate>()
                    .map_err(|e| CliError::Usage(e.to_string()))?,
            };
            let warehouse_id = uuid::Uuid::new_v4();
            let builder = buzz_sdk::build_crm_space_create(
                channel,
                settings,
                WarehouseSeed {
                    id: warehouse_id,
                    name: warehouse_name,
                },
            )
            .map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            print_create_response(&resp, "warehouse_id", &warehouse_id.to_string());
            Ok(())
        }
        CrmCmd::Warehouses(CrmWarehousesCmd::Create {
            channel,
            name,
            default,
        }) => {
            let channel = parse_uuid(&channel)?;
            let warehouse_id = uuid::Uuid::new_v4();
            let builder =
                buzz_sdk::build_crm_warehouse(channel, warehouse_id, &name, default, false, None)
                    .map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            print_create_response(&resp, "warehouse_id", &warehouse_id.to_string());
            Ok(())
        }
        CrmCmd::Warehouses(CrmWarehousesCmd::Update {
            channel,
            warehouse,
            revision,
            name,
            default,
            archived,
        }) => {
            let channel = parse_uuid(&channel)?;
            let warehouse_id = parse_uuid(&warehouse)?;
            let space: serde_json::Value =
                serde_json::from_str(&client.get_authed("/crm/space").await?)
                    .map_err(|e| CliError::Other(format!("unexpected /crm/space response: {e}")))?;
            let update = merge_warehouse_update(
                &space,
                &warehouse_id.to_string(),
                WarehouseFlags {
                    revision,
                    name,
                    default,
                    archived,
                },
            )?;
            let builder = buzz_sdk::build_crm_warehouse(
                channel,
                warehouse_id,
                &update.name,
                update.is_default,
                update.archived,
                Some(&update.revision),
            )
            .map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            println!("{}", crate::client::normalize_write_response(&resp));
            Ok(())
        }
    }
}

/// Flags of `buzz crm warehouses update`; `None` keeps the current value.
struct WarehouseFlags {
    revision: Option<String>,
    name: Option<String>,
    default: Option<bool>,
    archived: Option<bool>,
}

#[derive(Debug, PartialEq)]
struct WarehouseUpdate {
    revision: String,
    name: String,
    is_default: bool,
    archived: bool,
}

/// Fill unspecified flags from the warehouse's current state in `GET /crm/space`.
fn merge_warehouse_update(
    space: &serde_json::Value,
    warehouse_id: &str,
    flags: WarehouseFlags,
) -> Result<WarehouseUpdate, CliError> {
    let current = space["warehouses"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["id"] == warehouse_id))
        .ok_or_else(|| {
            CliError::Usage(format!(
                "warehouse {warehouse_id} is not an active warehouse of your CRM space"
            ))
        })?;
    let text = |key: &str| current[key].as_str().unwrap_or_default().to_string();
    let flag = |key: &str| current[key].as_bool().unwrap_or(false);
    Ok(WarehouseUpdate {
        revision: flags.revision.unwrap_or_else(|| text("revision")),
        name: flags.name.unwrap_or_else(|| text("name")),
        is_default: flags.default.unwrap_or_else(|| flag("is_default")),
        archived: flags.archived.unwrap_or_else(|| flag("archived")),
    })
}

fn parse_fiscalization(raw: &str) -> Result<FiscalizationMode, CliError> {
    match raw {
        "required" => Ok(FiscalizationMode::Required),
        "optional" => Ok(FiscalizationMode::Optional),
        "off" => Ok(FiscalizationMode::Off),
        other => Err(CliError::Usage(format!(
            "invalid --fiscalization {other:?} (expected required, optional or off)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space() -> serde_json::Value {
        serde_json::json!({
            "warehouses": [
                {"id": "w-main", "name": "Основной", "is_default": true, "archived": false, "revision": "r1"},
                {"id": "w-shop", "name": "Магазин", "is_default": false, "archived": false, "revision": "r2"}
            ]
        })
    }

    fn no_flags() -> WarehouseFlags {
        WarehouseFlags {
            revision: None,
            name: None,
            default: None,
            archived: None,
        }
    }

    #[test]
    fn renaming_the_default_keeps_it_default() {
        let update = merge_warehouse_update(
            &space(),
            "w-main",
            WarehouseFlags {
                name: Some("Главный".into()),
                ..no_flags()
            },
        )
        .unwrap();
        assert_eq!(
            update,
            WarehouseUpdate {
                revision: "r1".into(),
                name: "Главный".into(),
                is_default: true,
                archived: false,
            }
        );
    }

    #[test]
    fn explicit_flags_override_current_values() {
        let update = merge_warehouse_update(
            &space(),
            "w-shop",
            WarehouseFlags {
                revision: Some("r9".into()),
                default: Some(true),
                ..no_flags()
            },
        )
        .unwrap();
        assert_eq!(update.revision, "r9");
        assert!(update.is_default);
        assert_eq!(update.name, "Магазин");
    }

    #[test]
    fn unknown_warehouse_is_a_usage_error() {
        assert!(matches!(
            merge_warehouse_update(&space(), "w-missing", no_flags()),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn fiscalization_values() {
        assert_eq!(
            parse_fiscalization("required").unwrap(),
            FiscalizationMode::Required
        );
        assert_eq!(
            parse_fiscalization("optional").unwrap(),
            FiscalizationMode::Optional
        );
        assert_eq!(parse_fiscalization("off").unwrap(), FiscalizationMode::Off);
        assert!(matches!(
            parse_fiscalization("sometimes"),
            Err(CliError::Usage(_))
        ));
    }
}
