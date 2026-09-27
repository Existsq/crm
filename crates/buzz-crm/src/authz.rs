//! Role × action authorization for the CRM space (ADR-0002).
//!
//! Channel membership decides who can read; this table decides who can write.
//! The relay evaluates it inside the write transaction with the role read
//! from `channel_members`, and returns [`capabilities`] to clients so the UI
//! can hide actions the caller may not perform.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A member's role in the CRM space channel (mirrors `member_role`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrmRole {
    /// Channel owner.
    Owner,
    /// Channel admin.
    Admin,
    /// Regular member (staff).
    Member,
    /// Guest with scoped access.
    Guest,
    /// Agent / bot.
    Bot,
}

impl CrmRole {
    /// Every role.
    pub const ALL: [CrmRole; 5] = [
        CrmRole::Owner,
        CrmRole::Admin,
        CrmRole::Member,
        CrmRole::Guest,
        CrmRole::Bot,
    ];

    /// Canonical name, identical to the Postgres `member_role` enum value.
    pub const fn as_str(self) -> &'static str {
        match self {
            CrmRole::Owner => "owner",
            CrmRole::Admin => "admin",
            CrmRole::Member => "member",
            CrmRole::Guest => "guest",
            CrmRole::Bot => "bot",
        }
    }
}

impl FromStr for CrmRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CrmRole::ALL
            .into_iter()
            .find(|role| role.as_str() == s)
            .ok_or_else(|| format!("unknown member role {s:?}"))
    }
}

/// Something a caller may want to do in the CRM space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrmAction {
    /// Read any CRM data.
    Read,
    /// Change space settings.
    ManageSpace,
    /// Create, edit or archive warehouses.
    ManageWarehouses,
    /// Create or edit products.
    EditCatalog,
    /// Create or edit customers and suppliers.
    EditParties,
    /// Post purchase, sale, return, write-off and inventory documents.
    PostDocuments,
    /// Reverse (сторно) a posted document.
    VoidDocuments,
    /// Record fiscal receipts and cash-shift operations.
    RecordFiscal,
}

impl CrmAction {
    /// Every action, in a stable order.
    pub const ALL: [CrmAction; 8] = [
        CrmAction::Read,
        CrmAction::ManageSpace,
        CrmAction::ManageWarehouses,
        CrmAction::EditCatalog,
        CrmAction::EditParties,
        CrmAction::PostDocuments,
        CrmAction::VoidDocuments,
        CrmAction::RecordFiscal,
    ];

    /// Canonical wire name (`"edit_catalog"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            CrmAction::Read => "read",
            CrmAction::ManageSpace => "manage_space",
            CrmAction::ManageWarehouses => "manage_warehouses",
            CrmAction::EditCatalog => "edit_catalog",
            CrmAction::EditParties => "edit_parties",
            CrmAction::PostDocuments => "post_documents",
            CrmAction::VoidDocuments => "void_documents",
            CrmAction::RecordFiscal => "record_fiscal",
        }
    }
}

/// Whether `role` may perform `action`.
///
/// | Action | owner | admin | member | guest | bot |
/// |---|---|---|---|---|---|
/// | read | ✓ | ✓ | ✓ | ✓ | ✓ |
/// | manage space / warehouses, void | ✓ | ✓ | – | – | – |
/// | catalog, parties, documents | ✓ | ✓ | ✓ | – | ✓ |
/// | fiscal receipts, cash shift | ✓ | ✓ | ✓ | – | – |
pub const fn decide(role: CrmRole, action: CrmAction) -> bool {
    use CrmAction::*;
    use CrmRole::*;
    match action {
        Read => true,
        ManageSpace | ManageWarehouses | VoidDocuments => matches!(role, Owner | Admin),
        EditCatalog | EditParties | PostDocuments => matches!(role, Owner | Admin | Member | Bot),
        RecordFiscal => matches!(role, Owner | Admin | Member),
    }
}

/// Every action `role` may perform, in [`CrmAction::ALL`] order.
pub fn capabilities(role: CrmRole) -> Vec<CrmAction> {
    CrmAction::ALL
        .into_iter()
        .filter(|action| decide(role, *action))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full matrix from ADR-0002, written out independently of `decide`.
    const EXPECTED: [(CrmRole, [bool; 8]); 5] = [
        //                read  space  wh     catal  party  post   void   fiscal
        (
            CrmRole::Owner,
            [true, true, true, true, true, true, true, true],
        ),
        (
            CrmRole::Admin,
            [true, true, true, true, true, true, true, true],
        ),
        (
            CrmRole::Member,
            [true, false, false, true, true, true, false, true],
        ),
        (
            CrmRole::Guest,
            [true, false, false, false, false, false, false, false],
        ),
        (
            CrmRole::Bot,
            [true, false, false, true, true, true, false, false],
        ),
    ];

    #[test]
    fn decision_matrix_matches_adr() {
        for (role, row) in EXPECTED {
            for (action, expected) in CrmAction::ALL.into_iter().zip(row) {
                assert_eq!(
                    decide(role, action),
                    expected,
                    "{} × {}",
                    role.as_str(),
                    action.as_str()
                );
            }
        }
    }

    #[test]
    fn capabilities_follow_decide() {
        assert_eq!(capabilities(CrmRole::Guest), vec![CrmAction::Read]);
        assert_eq!(capabilities(CrmRole::Owner), CrmAction::ALL.to_vec());
        assert!(!capabilities(CrmRole::Bot).contains(&CrmAction::RecordFiscal));
    }

    #[test]
    fn role_names_match_postgres_enum() {
        for role in CrmRole::ALL {
            assert_eq!(role.as_str().parse::<CrmRole>().unwrap(), role);
        }
        assert!("superuser".parse::<CrmRole>().is_err());
    }
}
