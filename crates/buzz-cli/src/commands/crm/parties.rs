//! `buzz crm parties` — customers and suppliers (kind 42103).

use buzz_crm::{PartyPayload, PartyType};
use serde::Deserialize;

use crate::client::{create_response_with_id_if_accepted, normalize_write_response, BuzzClient};
use crate::error::CliError;
use crate::validate::{parse_uuid, sdk_err};
use crate::{CrmPartiesCmd, CrmPartyFields};

/// Run a `buzz crm parties` subcommand and return the JSON it prints.
pub async fn run(cmd: CrmPartiesCmd, client: &BuzzClient) -> Result<String, CliError> {
    match cmd {
        CrmPartiesCmd::List {
            query,
            role,
            archived,
            limit,
            after,
        } => {
            let mut params = url::form_urlencoded::Serializer::new(String::new());
            if let Some(q) = query.as_deref() {
                params.append_pair("q", q);
            }
            if let Some(role) = role.as_deref() {
                params.append_pair("role", role);
            }
            if archived {
                params.append_pair("archived", "true");
            }
            if let Some(limit) = limit {
                params.append_pair("limit", &limit.to_string());
            }
            if let Some(after) = after.as_deref() {
                params.append_pair("after", after);
            }
            client
                .get_authed(&with_query("/crm/parties", params.finish()))
                .await
        }
        CrmPartiesCmd::Get { party } => {
            let party = parse_uuid(&party)?;
            client.get_authed(&format!("/crm/parties/{party}")).await
        }
        CrmPartiesCmd::Create { channel, fields } => {
            let channel = parse_uuid(&channel)?;
            let name = fields
                .name
                .clone()
                .ok_or_else(|| CliError::Usage("--name is required".into()))?;
            let base = CurrentParty {
                party_type: PartyType::Customer,
                name,
                phone: None,
                email: None,
                inn: None,
                address: None,
                note: None,
                archived: false,
                revision: String::new(),
            };
            let payload = merge_party(base, fields, None)?;
            let party_id = uuid::Uuid::new_v4();
            let builder =
                buzz_sdk::build_crm_party(channel, party_id, payload, None).map_err(sdk_err)?;
            let resp = client.submit_event(client.sign_event(builder)?).await?;
            Ok(create_response_with_id_if_accepted(
                &resp,
                "party_id",
                &party_id.to_string(),
            ))
        }
        CrmPartiesCmd::Update {
            channel,
            party,
            revision,
            fields,
        } => {
            let channel = parse_uuid(&channel)?;
            let party = parse_uuid(&party)?;
            let current = current_party(client, party).await?;
            let revision = revision.unwrap_or_else(|| current.revision.clone());
            let payload = merge_party(current, fields, None)?;
            submit_update(client, channel, party, payload, &revision).await
        }
        CrmPartiesCmd::Archive {
            channel,
            party,
            restore,
        } => {
            let channel = parse_uuid(&channel)?;
            let party = parse_uuid(&party)?;
            let current = current_party(client, party).await?;
            let revision = current.revision.clone();
            let payload = merge_party(current, CrmPartyFields::default(), Some(!restore))?;
            submit_update(client, channel, party, payload, &revision).await
        }
    }
}

pub(super) fn with_query(path: &str, encoded: String) -> String {
    if encoded.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{encoded}")
    }
}

async fn submit_update(
    client: &BuzzClient,
    channel: uuid::Uuid,
    party: uuid::Uuid,
    payload: PartyPayload,
    revision: &str,
) -> Result<String, CliError> {
    let builder =
        buzz_sdk::build_crm_party(channel, party, payload, Some(revision)).map_err(sdk_err)?;
    let resp = client.submit_event(client.sign_event(builder)?).await?;
    Ok(normalize_write_response(&resp))
}

/// The party as `GET /crm/parties/{id}` returns it.
#[derive(Debug, Deserialize)]
struct CurrentParty {
    #[serde(rename = "type")]
    party_type: PartyType,
    name: String,
    phone: Option<String>,
    email: Option<String>,
    inn: Option<String>,
    address: Option<String>,
    note: Option<String>,
    archived: bool,
    revision: String,
}

async fn current_party(client: &BuzzClient, party: uuid::Uuid) -> Result<CurrentParty, CliError> {
    let body = client.get_authed(&format!("/crm/parties/{party}")).await?;
    serde_json::from_str(&body)
        .map_err(|e| CliError::Other(format!("unexpected /crm/parties response: {e}")))
}

/// Overlay the given flags on `current`; `""` clears an optional field and
/// `archived` overrides the flag.
fn merge_party(
    current: CurrentParty,
    fields: CrmPartyFields,
    archived: Option<bool>,
) -> Result<PartyPayload, CliError> {
    let text = |flag: Option<String>, current: Option<String>| match flag {
        Some(raw) => (!raw.trim().is_empty()).then_some(raw),
        None => current,
    };
    let party_type = match fields.party_type {
        Some(raw) => serde_json::from_value(serde_json::Value::String(raw.clone()))
            .map_err(|_| CliError::Usage(format!("invalid --type {raw:?}")))?,
        None => current.party_type,
    };
    Ok(PartyPayload {
        v: buzz_crm::space::PAYLOAD_VERSION,
        party_type,
        name: fields.name.unwrap_or(current.name),
        phone: text(fields.phone, current.phone),
        email: text(fields.email, current.email),
        inn: text(fields.inn, current.inn),
        address: text(fields.address, current.address),
        note: text(fields.note, current.note),
        archived: archived.unwrap_or(current.archived),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current() -> CurrentParty {
        CurrentParty {
            party_type: PartyType::Supplier,
            name: "Молокозавод".into(),
            phone: Some("+7 342 000-00-00".into()),
            email: None,
            inn: Some("7707083893".into()),
            address: None,
            note: Some("Поставки по вторникам".into()),
            archived: true,
            revision: "r1".into(),
        }
    }

    #[test]
    fn omitted_flags_keep_values_and_empty_flags_clear_them() {
        let payload = merge_party(
            current(),
            CrmPartyFields {
                party_type: Some("both".into()),
                note: Some(String::new()),
                ..CrmPartyFields::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(payload.party_type, PartyType::Both);
        assert_eq!(payload.name, "Молокозавод");
        assert_eq!(payload.inn.as_deref(), Some("7707083893"));
        assert_eq!(payload.note, None);
        assert!(payload.archived);
    }

    #[test]
    fn archive_flag_overrides_and_bad_type_is_usage_error() {
        let restored = merge_party(current(), CrmPartyFields::default(), Some(false)).unwrap();
        assert!(!restored.archived);
        let bad = CrmPartyFields {
            party_type: Some("partner".into()),
            ..CrmPartyFields::default()
        };
        assert!(matches!(
            merge_party(current(), bad, None),
            Err(CliError::Usage(_))
        ));
    }
}
