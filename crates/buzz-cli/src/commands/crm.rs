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
            let builder = buzz_sdk::build_crm_warehouse(
                channel,
                warehouse_id,
                &name,
                default,
                archived,
                Some(&revision),
            )
            .map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            println!("{}", crate::client::normalize_write_response(&resp));
            Ok(())
        }
    }
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
