//! This module defines the type to configure variants which can restrict Agent Type values to a particular
//! collection of supported values.

use crate::agent_type::variable::constraints::VariantsConstraints;
use serde::{Deserialize, Serialize};
use tracing::debug;

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
    pub fn is_valid(&self, value: &str) -> bool {
        self.0.is_empty() || self.0.iter().any(|v| v == value)
    }

    /// Resolves the set of valid variants for a `string` variable, considering the Agent Control
    /// configuration overrides pointed to by ac_config_field.
    pub fn new(
        variants_config: &VariantsConfig,
        variants_constraints: &VariantsConstraints,
    ) -> Self {
        let Some(ac_config_field) = variants_config.ac_config_field.as_ref() else {
            return variants_config.values.clone();
        };

        let Some(supported_values) = variants_constraints.get(ac_config_field) else {
            debug!(
                %ac_config_field,
                "The variants pointed in Agent Type are not set in Agent Control configuration, using defaults"
            );
            return variants_config.values.clone();
        };

        supported_values.into()
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
    use crate::agent_type::variable::constraints::VariableConstraints;
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

    #[rstest]
    #[case::no_variants_default(
        VariantsConfig::default(),
        r#"{"variants": {}}"#,
        Variants::default()
    )]
    #[case::variants_with_no_match_with_no_values(
        VariantsConfig { ac_config_field: Some("some_key".to_string()), values: Default::default() },
        r#"{"variants": {"other_key": ["a", "b"]}}"#,
        Variants::default()
    )]
    #[case::variants_with_no_match_with_values(
        VariantsConfig { ac_config_field: Some("some_key".to_string()), values: vec!["x".to_string()].into() },
        r#"{"variants": {"other_key": ["a", "b"]}}"#,
        vec!["x".to_string()].into()
    )]
    #[case::variants_with_match_with_no_values(
        VariantsConfig { ac_config_field: Some("some_key".to_string()), values: Default::default() },
        r#"{"variants": {"some_key": ["a", "b"]}}"#,
        vec!["a".to_string(), "b".to_string()].into()
    )]
    #[case::variants_with_match_with_values(
        VariantsConfig { ac_config_field: Some("some_key".to_string()), values: vec!["x".to_string()].into() },
        r#"{"variants": {"some_key": ["a", "b"]}}"#,
        vec!["a".to_string(), "b".to_string()].into()
    )]
    fn build_variants_resolves_against_ac_constraints(
        #[case] variants: VariantsConfig,
        #[case] constraints_json: &str,
        #[case] expected: Variants,
    ) {
        let constraints: VariableConstraints = serde_json::from_str(constraints_json).unwrap();
        let actual = Variants::new(&variants, &constraints.variants);
        assert_eq!(actual, expected);
    }
}
