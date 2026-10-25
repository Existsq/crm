//! Pure CRM authorization decisions, table-tested without a database.

use buzz_crm::{CrmAction, CrmRole};
use buzz_db::crm::CrmChannelFacts;

use super::super::ingest::IngestError;

/// Whether `facts` describe a channel that may host (or keep hosting) a CRM
/// space: an existing private stream channel that is not archived and has no
/// TTL.
pub(super) fn channel_can_host_space(facts: Option<&CrmChannelFacts>) -> Result<(), IngestError> {
    let facts = facts.ok_or_else(|| IngestError::Rejected("invalid: channel not found".into()))?;
    if facts.channel_type != "stream" {
        return Err(IngestError::Rejected(
            "invalid: a CRM space must be a stream channel".into(),
        ));
    }
    if facts.visibility != "private" {
        return Err(IngestError::Rejected(
            "invalid: a CRM space channel must be private".into(),
        ));
    }
    if facts.archived {
        return Err(IngestError::Rejected(
            "invalid: a CRM space channel must not be archived".into(),
        ));
    }
    if facts.has_ttl {
        return Err(IngestError::Rejected(
            "invalid: a CRM space channel must not expire (ttl)".into(),
        ));
    }
    Ok(())
}

/// Creating a space needs a community owner/admin who is also owner/admin of
/// the channel being bound.
pub(super) fn may_create_space(
    relay_role: Option<&str>,
    channel_role: Option<&str>,
) -> Result<(), IngestError> {
    if !matches!(relay_role, Some("owner" | "admin")) {
        return Err(IngestError::AuthFailed(
            "restricted: only community owners and admins can create a CRM space".into(),
        ));
    }
    if !matches!(channel_role, Some("owner" | "admin")) {
        return Err(IngestError::AuthFailed(
            "restricted: only the channel owner or an admin can bind it as the CRM space".into(),
        ));
    }
    Ok(())
}

/// Role × action check for writes into an existing space.
pub(super) fn authorize(channel_role: Option<&str>, action: CrmAction) -> Result<(), IngestError> {
    let role = channel_role
        .ok_or_else(|| IngestError::AuthFailed("restricted: not a member of the CRM space".into()))?
        .parse::<CrmRole>()
        .map_err(|e| IngestError::Internal(format!("error: {e}")))?;
    if buzz_crm::decide(role, action) {
        Ok(())
    } else {
        Err(IngestError::AuthFailed(format!(
            "restricted: role {} may not {}",
            role.as_str(),
            action.as_str()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(
        channel_type: &str,
        visibility: &str,
        archived: bool,
        has_ttl: bool,
    ) -> CrmChannelFacts {
        CrmChannelFacts {
            channel_type: channel_type.into(),
            visibility: visibility.into(),
            archived,
            has_ttl,
        }
    }

    fn message(result: Result<(), IngestError>) -> Option<String> {
        match result {
            Ok(()) => None,
            Err(IngestError::Rejected(m) | IngestError::AuthFailed(m)) => Some(m),
            Err(IngestError::Internal(m) | IngestError::CanvasConflict(m)) => Some(m),
        }
    }

    #[test]
    fn only_private_permanent_live_stream_channels_host_spaces() {
        assert_eq!(
            message(channel_can_host_space(Some(&facts(
                "stream", "private", false, false
            )))),
            None
        );
        let cases = [
            (None, "invalid: channel not found"),
            (
                Some(facts("forum", "private", false, false)),
                "invalid: a CRM space must be a stream channel",
            ),
            (
                Some(facts("dm", "private", false, false)),
                "invalid: a CRM space must be a stream channel",
            ),
            (
                Some(facts("stream", "open", false, false)),
                "invalid: a CRM space channel must be private",
            ),
            (
                Some(facts("stream", "private", true, false)),
                "invalid: a CRM space channel must not be archived",
            ),
            (
                Some(facts("stream", "private", false, true)),
                "invalid: a CRM space channel must not expire (ttl)",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                message(channel_can_host_space(input.as_ref())).as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn space_creation_needs_both_community_and_channel_admin() {
        let relay_roles = [None, Some("member"), Some("admin"), Some("owner")];
        let channel_roles = [
            None,
            Some("guest"),
            Some("bot"),
            Some("member"),
            Some("admin"),
            Some("owner"),
        ];
        for relay in relay_roles {
            for channel in channel_roles {
                let allowed = matches!(relay, Some("owner" | "admin"))
                    && matches!(channel, Some("owner" | "admin"));
                let result = may_create_space(relay, channel);
                assert_eq!(
                    result.is_ok(),
                    allowed,
                    "relay={relay:?} channel={channel:?}"
                );
                if let Err(error) = result {
                    assert!(
                        matches!(error, IngestError::AuthFailed(m) if m.starts_with("restricted:"))
                    );
                }
            }
        }
    }

    #[test]
    fn authorize_follows_the_crm_table_and_rejects_non_members() {
        for role in CrmRole::ALL {
            for action in CrmAction::ALL {
                assert_eq!(
                    authorize(Some(role.as_str()), action).is_ok(),
                    buzz_crm::decide(role, action),
                    "{} × {}",
                    role.as_str(),
                    action.as_str()
                );
            }
        }
        assert!(matches!(
            authorize(None, CrmAction::Read),
            Err(IngestError::AuthFailed(m)) if m == "restricted: not a member of the CRM space"
        ));
    }
}
