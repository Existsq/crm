//! Error type for CRM validation.

/// A CRM payload or tag failed validation.
///
/// The `Display` text is a human-readable reason without a prefix; the relay
/// prepends `invalid:` when it rejects an event.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CrmError {
    /// The event content is not valid JSON for the expected payload.
    #[error("malformed CRM payload: {0}")]
    MalformedPayload(String),
    /// The payload declares a schema version this build does not understand.
    #[error("unsupported CRM payload version {0}")]
    UnsupportedVersion(u64),
    /// A tag is missing, duplicated or malformed.
    #[error("{0}")]
    Tag(String),
    /// A field value is out of bounds or otherwise invalid.
    #[error("{field}: {reason}")]
    Field {
        /// Payload field path, e.g. `settings.timezone`.
        field: &'static str,
        /// Why the value was rejected.
        reason: String,
    },
    /// An arithmetic result does not fit the domain bounds.
    #[error("amount out of range")]
    Overflow,
}

impl CrmError {
    /// Build a [`CrmError::Field`] error.
    pub fn field(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Field {
            field,
            reason: reason.into(),
        }
    }
}

/// Result alias for CRM validation.
pub type Result<T> = std::result::Result<T, CrmError>;
