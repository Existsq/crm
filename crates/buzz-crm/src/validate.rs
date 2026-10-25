//! Field validators shared by every CRM payload.

use crate::error::{CrmError, Result};

/// Maximum length (in characters) of names: products, warehouses, spaces.
///
/// 128 is the fiscal item-name limit, so a product name always fits a receipt.
pub const MAX_NAME_CHARS: usize = 128;

/// Trim and validate a display name: 1–[`MAX_NAME_CHARS`] characters, no
/// control characters.
pub fn name(value: &str, field: &'static str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(CrmError::field(field, "must not be empty"));
    }
    if trimmed.chars().count() > MAX_NAME_CHARS {
        return Err(CrmError::field(
            field,
            format!("must be at most {MAX_NAME_CHARS} characters"),
        ));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(CrmError::field(
            field,
            "must not contain control characters",
        ));
    }
    Ok(trimmed.to_owned())
}

/// Validate a Russian taxpayer number (ИНН): 10 digits for organisations,
/// 12 digits for individuals, with the check digits defined by the Federal
/// Tax Service.
pub fn inn(value: &str) -> Result<()> {
    let digits: Vec<u32> = value
        .chars()
        .map(|c| c.to_digit(10))
        .collect::<Option<_>>()
        .ok_or_else(|| CrmError::field("inn", "must contain only digits"))?;
    let check = |weights: &[u32]| -> u32 {
        weights
            .iter()
            .zip(&digits)
            .map(|(weight, digit)| weight * digit)
            .sum::<u32>()
            % 11
            % 10
    };
    let valid = match digits.len() {
        10 => check(&[2, 4, 10, 3, 5, 9, 4, 6, 8]) == digits[9],
        12 => {
            check(&[7, 2, 4, 10, 3, 5, 9, 4, 6, 8]) == digits[10]
                && check(&[3, 7, 2, 4, 10, 3, 5, 9, 4, 6, 8]) == digits[11]
        }
        _ => return Err(CrmError::field("inn", "must have 10 or 12 digits")),
    };
    if valid {
        Ok(())
    } else {
        Err(CrmError::field("inn", "checksum mismatch"))
    }
}

/// Shape check for an IANA timezone name (`Europe/Moscow`, `UTC`).
///
/// The relay additionally confirms the name against `pg_timezone_names`
/// inside the write transaction, so analytics never meets an unknown zone.
pub fn timezone(value: &str) -> Result<()> {
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment.len() <= 32
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'))
    };
    let segments: Vec<&str> = value.split('/').collect();
    if value.len() > 64 || segments.len() > 3 || !segments.iter().all(|s| valid_segment(s)) {
        return Err(CrmError::field(
            "settings.timezone",
            format!("{value:?} is not an IANA timezone name"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(name("  Склад  ", "name").unwrap(), "Склад");
        assert!(name("   ", "name").is_err());
        assert!(name(&"я".repeat(MAX_NAME_CHARS), "name").is_ok());
        assert!(name(&"я".repeat(MAX_NAME_CHARS + 1), "name").is_err());
        assert!(name("line\nbreak", "name").is_err());
    }

    #[test]
    fn inn_checksums() {
        // Real registered organisation and individual numbers.
        for valid in ["7707083893", "7736207543", "500100732259"] {
            assert!(inn(valid).is_ok(), "{valid} should be valid");
        }
        for invalid in ["7707083894", "500100732258", "772170412837"] {
            assert_eq!(
                inn(invalid),
                Err(CrmError::field("inn", "checksum mismatch")),
                "{invalid}"
            );
        }
        assert!(inn("12345").is_err());
        assert!(inn("77070838a3").is_err());
    }

    #[test]
    fn timezone_shapes() {
        for ok in [
            "UTC",
            "Europe/Moscow",
            "America/Argentina/Buenos_Aires",
            "Etc/GMT+3",
        ] {
            assert!(timezone(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "Europe/",
            "/Moscow",
            "Europe/Moscow; DROP",
            "a/b/c/d",
            "Europe Moscow",
        ] {
            assert!(timezone(bad).is_err(), "{bad:?}");
        }
    }
}
