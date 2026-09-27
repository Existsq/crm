//! `buzz crm` — retail CRM space, warehouses and products (kinds 42100–42199).

mod products;

use buzz_crm::{Currency, FiscalizationMode, SpaceSettings, VatRate, WarehouseSeed};

use crate::client::{create_response_with_id_if_accepted, normalize_write_response, BuzzClient};
use crate::error::CliError;
use crate::validate::{parse_uuid, sdk_err};
use crate::{CrmCmd, CrmSpaceCmd, CrmWarehousesCmd};

/// Dispatch a `buzz crm` subcommand and print its JSON output.
pub async fn dispatch(cmd: CrmCmd, client: &BuzzClient) -> Result<(), CliError> {
    println!("{}", run(cmd, client).await?);
    Ok(())
}

/// Run a `buzz crm` subcommand and return the JSON it prints.
async fn run(cmd: CrmCmd, client: &BuzzClient) -> Result<String, CliError> {
    match cmd {
        CrmCmd::Products(cmd) => products::run(cmd, client).await,
        CrmCmd::Space(CrmSpaceCmd::Show) => client.get_authed("/crm/space").await,
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
            Ok(create_response_with_id_if_accepted(
                &resp,
                "warehouse_id",
                &warehouse_id.to_string(),
            ))
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
            Ok(create_response_with_id_if_accepted(
                &resp,
                "warehouse_id",
                &warehouse_id.to_string(),
            ))
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
            // Archived warehouses are included so `--archived false` can restore one.
            let space: serde_json::Value =
                serde_json::from_str(&client.get_authed("/crm/space?archived=true").await?)
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
            Ok(normalize_write_response(&resp))
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
                "warehouse {warehouse_id} is not a warehouse of your CRM space"
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

    // -----------------------------------------------------------------------
    // Real client seam against a fake relay (axum)
    // -----------------------------------------------------------------------

    use axum::extract::State;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use nostr::Keys;
    use std::sync::{Arc, Mutex};

    type Posted = Arc<Mutex<Vec<nostr::Event>>>;

    /// Serves `GET /crm/space` with `space_body` (archived warehouses only
    /// for `?archived=true`, like the relay) and accepts `POST /events`,
    /// recording each submitted event.
    async fn fake_relay(space_body: serde_json::Value) -> (BuzzClient, Posted) {
        let posted: Posted = Arc::default();
        let app = Router::new()
            .route(
                "/crm/space",
                get(
                    move |axum::extract::RawQuery(query): axum::extract::RawQuery| {
                        let mut body = space_body.clone();
                        if query.as_deref() != Some("archived=true") {
                            if let Some(rows) = body
                                .get_mut("warehouses")
                                .and_then(serde_json::Value::as_array_mut)
                            {
                                rows.retain(|row| row["archived"] != serde_json::json!(true));
                            }
                        }
                        async move { Json(body) }
                    },
                ),
            )
            .route(
                "/events",
                post(
                    |State(posted): State<Posted>, Json(event): Json<nostr::Event>| async move {
                        let id = event.id.to_hex();
                        posted.lock().unwrap().push(event);
                        Json(serde_json::json!({"event_id": id, "accepted": true, "message": ""}))
                    },
                ),
            )
            .with_state(posted.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client =
            BuzzClient::new(format!("http://{addr}"), Keys::generate(), None, None).unwrap();
        (client, posted)
    }

    #[tokio::test]
    async fn space_show_prints_the_relay_response() {
        let body = serde_json::json!({"exists": false, "can_create": true});
        let (client, _) = fake_relay(body.clone()).await;
        let out = run(CrmCmd::Space(CrmSpaceCmd::Show), &client)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&out).unwrap(),
            body
        );
    }

    #[tokio::test]
    async fn space_init_publishes_a_space_event_and_prints_the_warehouse_id() {
        let (client, posted) = fake_relay(serde_json::json!({})).await;
        let channel = uuid::Uuid::new_v4();
        let out = run(
            CrmCmd::Space(CrmSpaceCmd::Init {
                channel: channel.to_string(),
                timezone: "Asia/Yekaterinburg".into(),
                fiscalization: "off".into(),
                default_vat: "vat10".into(),
                allow_negative_stock: true,
                warehouse_name: "Склад 1".into(),
            }),
            &client,
        )
        .await
        .unwrap();

        let events = posted.lock().unwrap().clone();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.kind.as_u16(), 42100);
        let h: Vec<&str> = event
            .tags
            .iter()
            .filter(|t| t.as_slice().first().map(String::as_str) == Some("h"))
            .filter_map(|t| t.as_slice().get(1).map(String::as_str))
            .collect();
        assert_eq!(h, vec![channel.to_string()]);
        let payload = buzz_crm::SpacePayload::parse(&event.content, false).unwrap();
        assert_eq!(payload.settings.timezone, "Asia/Yekaterinburg");
        assert_eq!(payload.settings.fiscalization, FiscalizationMode::Off);
        assert_eq!(payload.settings.default_vat, VatRate::Vat10);
        assert!(payload.settings.allow_negative_stock);
        let seed = payload.default_warehouse.unwrap();
        assert_eq!(seed.name, "Склад 1");

        let printed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(printed["event_id"], serde_json::json!(event.id.to_hex()));
        assert_eq!(printed["accepted"], serde_json::json!(true));
        assert_eq!(
            printed["warehouse_id"],
            serde_json::json!(seed.id.to_string())
        );
    }

    #[tokio::test]
    async fn warehouse_update_can_restore_an_archived_warehouse() {
        let space = serde_json::json!({
            "warehouses": [
                {"id": "00000000-0000-0000-0000-000000000001", "name": "Старый",
                 "is_default": false, "archived": true, "revision": "ab".repeat(32)}
            ]
        });
        let (client, posted) = fake_relay(space).await;
        run(
            CrmCmd::Warehouses(CrmWarehousesCmd::Update {
                channel: uuid::Uuid::new_v4().to_string(),
                warehouse: "00000000-0000-0000-0000-000000000001".into(),
                revision: None,
                name: None,
                default: None,
                archived: Some(false),
            }),
            &client,
        )
        .await
        .unwrap();
        let events = posted.lock().unwrap().clone();
        let payload = buzz_crm::WarehousePayload::parse(&events[0].content).unwrap();
        assert!(!payload.archived);
        assert_eq!(payload.name, "Старый");
        let revision = events[0]
            .tags
            .iter()
            .find(|t| t.as_slice().first().map(String::as_str) == Some("expected-revision"))
            .and_then(|t| t.as_slice().get(1).cloned());
        assert_eq!(
            revision,
            Some("ab".repeat(32)),
            "CAS on the archived row's revision"
        );
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
