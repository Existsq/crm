//! Tag conventions shared by every CRM event.
//!
//! - `["h", <space channel uuid>]` — the CRM space channel (checked by the relay).
//! - `["crm-id", <entity uuid>]` — the product/warehouse/party/document id.
//! - `["expected-revision", <64-hex event id>]` — CAS precondition on updates.

use uuid::Uuid;

use crate::error::{CrmError, Result};

/// Tag carrying the entity UUID.
pub const TAG_CRM_ID: &str = "crm-id";
/// Tag carrying the optimistic-concurrency precondition.
pub const TAG_EXPECTED_REVISION: &str = "expected-revision";

/// Return the value of the single tag named `name`, `None` when absent.
///
/// A repeated tag is ambiguous and rejected, as is a tag without a value.
pub fn single<'a, I, T>(tags: I, name: &str) -> Result<Option<&'a str>>
where
    I: IntoIterator<Item = &'a T>,
    T: AsRef<[String]> + 'a + ?Sized,
{
    let mut found: Option<&'a str> = None;
    for tag in tags {
        let parts = tag.as_ref();
        if parts.first().map(String::as_str) != Some(name) {
            continue;
        }
        let value = parts
            .get(1)
            .map(String::as_str)
            .ok_or_else(|| CrmError::Tag(format!("{name} tag has no value")))?;
        if found.is_some() {
            return Err(CrmError::Tag(format!("duplicate {name} tag")));
        }
        found = Some(value);
    }
    Ok(found)
}

/// The required `crm-id` tag, parsed as a UUID.
pub fn crm_id<'a, I, T>(tags: I) -> Result<Uuid>
where
    I: IntoIterator<Item = &'a T>,
    T: AsRef<[String]> + 'a + ?Sized,
{
    let raw = single(tags, TAG_CRM_ID)?
        .ok_or_else(|| CrmError::Tag(format!("missing {TAG_CRM_ID} tag")))?;
    parse_uuid(raw, TAG_CRM_ID)
}

/// The optional `expected-revision` tag, parsed as a 32-byte event id.
pub fn expected_revision<'a, I, T>(tags: I) -> Result<Option<[u8; 32]>>
where
    I: IntoIterator<Item = &'a T>,
    T: AsRef<[String]> + 'a + ?Sized,
{
    single(tags, TAG_EXPECTED_REVISION)?
        .map(parse_event_id)
        .transpose()
}

/// Parse a canonical lowercase-hyphenated UUID.
pub fn parse_uuid(raw: &str, what: &str) -> Result<Uuid> {
    let parsed =
        Uuid::parse_str(raw).map_err(|_| CrmError::Tag(format!("{what} must be a UUID")))?;
    if parsed.is_nil() || parsed.hyphenated().to_string() != raw {
        return Err(CrmError::Tag(format!(
            "{what} must be a non-nil lowercase hyphenated UUID"
        )));
    }
    Ok(parsed)
}

/// Parse a 64-character lowercase hex event id.
pub fn parse_event_id(raw: &str) -> Result<[u8; 32]> {
    if raw.len() != 64 || !raw.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
        return Err(CrmError::Tag(format!(
            "{TAG_EXPECTED_REVISION} must be a 64-char lowercase hex event id"
        )));
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16)
            .map_err(|_| CrmError::Tag(format!("{TAG_EXPECTED_REVISION} is not hex")))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(raw: &[&[&str]]) -> Vec<Vec<String>> {
        raw.iter()
            .map(|parts| parts.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    #[test]
    fn single_rejects_duplicates_and_empty_values() {
        let t = tags(&[&["crm-id", "a"], &["crm-id", "b"]]);
        assert!(single(&t, "crm-id").is_err());
        let t = tags(&[&["crm-id"]]);
        assert!(single(&t, "crm-id").is_err());
        let t = tags(&[&["h", "x"]]);
        assert_eq!(single(&t, "crm-id").unwrap(), None);
    }

    #[test]
    fn crm_id_requires_canonical_uuid() {
        let id = Uuid::new_v4();
        let t = tags(&[&["crm-id", &id.to_string()]]);
        assert_eq!(crm_id(&t).unwrap(), id);
        let upper = id.to_string().to_uppercase();
        assert!(crm_id(&tags(&[&["crm-id", &upper]])).is_err());
        assert!(crm_id(&tags(&[&["crm-id", &Uuid::nil().to_string()]])).is_err());
        assert!(crm_id(&tags(&[])).is_err());
    }

    #[test]
    fn expected_revision_parses_hex() {
        let hex = "ab".repeat(32);
        let t = tags(&[&["expected-revision", &hex]]);
        assert_eq!(expected_revision(&t).unwrap(), Some([0xab; 32]));
        assert_eq!(expected_revision(&tags(&[])).unwrap(), None);
        assert!(expected_revision(&tags(&[&["expected-revision", "AB"]])).is_err());
        let upper = "AB".repeat(32);
        assert!(expected_revision(&tags(&[&["expected-revision", &upper]])).is_err());
    }
}
