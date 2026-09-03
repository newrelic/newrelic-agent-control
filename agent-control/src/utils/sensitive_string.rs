//! A string wrapper for values that must never be printed, such as credentials and tokens.

use serde::Deserialize;
use std::fmt;

/// Holds a string value that must never appear in logs or debug output. Its [`Debug`]
/// implementation always prints `[REDACTED]` regardless of the wrapped value; use
/// [`SensitiveString::expose_secret`] to access the value itself.
///
/// It does **not** implement [serde::Serialize] and [fmt::Display] to force any types
/// using it to use [Self::expose_secret] or redact it properly.
#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
pub struct SensitiveString(String);

impl fmt::Debug for SensitiveString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl SensitiveString {
    /// Returns the wrapped value. Callers must not log, print, or otherwise expose the result.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl From<String> for SensitiveString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SensitiveString {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_redacts_value() {
        let secret = SensitiveString::from("super-secret-token");
        assert_eq!(format!("{secret:?}"), "[REDACTED]");
    }

    #[test]
    fn test_expose_secret_returns_value() {
        let secret = SensitiveString::from("super-secret-token");
        assert_eq!(secret.expose_secret(), "super-secret-token");
    }
}
