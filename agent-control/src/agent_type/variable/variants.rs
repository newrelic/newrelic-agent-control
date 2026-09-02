//! This module defines the type to configure variants which can restrict Agent Type values to a particular
//! collection of supported values.

use serde::{Deserialize, Serialize};

/// Represents a collection of supported variants for a variable.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct Variants(Vec<String>);

/// Defines the configuration to be set when defining [Variants] from Agent Control configuration.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct VariantsConfig {
    #[serde(default)]
    pub(crate) ac_config_field: Option<String>,
    #[serde(default)]
    pub(crate) values: Variants,
}

impl Variants {
    /// Returns whether `value` is allowed: true if there are no restrictions, or if `value` is one
    /// of the configured variants.
    pub fn is_valid(&self, value: &String) -> bool {
        self.0.is_empty() || self.0.iter().any(|v| v == value)
    }
}

impl From<Vec<String>> for Variants {
    fn from(value: Vec<String>) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for Variants {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]", self.0.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::default("", Default::default())]
    #[case::values_only(
        r#"{"values": ["v"]}"#,
        VariantsConfig { values: vec!["v".to_string()].into(), ..Default::default()})
    ]
    #[case::values_only(
        r#"{"ac_config_field": "some_variants"}"#,
        VariantsConfig { ac_config_field: Some("some_variants".to_string()), ..Default::default()})
    ]
    #[case::all(
        r#"{"ac_config_field": "some_variants", "values": ["v1", "v2"]}"#,
        VariantsConfig { ac_config_field: Some("some_variants".to_string()), values: vec!["v1".to_string(), "v2".to_string()].into()})
    ]
    fn test_variants_config_deserialization(#[case] input: &str, #[case] expected: VariantsConfig) {
        let value: VariantsConfig = serde_saphyr::from_str(input).unwrap();
        assert_eq!(value, expected);
    }
}
