//! This module defines the type to configure variants which can restrict Agent Type values to a particular
//! collection of supported values.

use serde::{Deserialize, Serialize};

/// Represents a collection of supported variants for a variable.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Variants<T: PartialEq>(pub(crate) Vec<T>); // TODO: we may not need it to be public

/// Defines the configuration to be set when defining [Variants] from Agent Control configuration.
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize)]
pub struct VariantsConfig<T>
where
    T: PartialEq,
{
    #[serde(default)]
    pub(crate) ac_config_field: Option<String>,
    #[serde(default)]
    pub(crate) values: Variants<T>,
}

impl<T> Variants<T>
where
    T: PartialEq,
{
    pub fn is_valid(&self, value: &T) -> bool {
        self.0.is_empty() || self.0.iter().any(|v| v == value)
    }
}

impl<T> From<Vec<T>> for Variants<T>
where
    T: PartialEq,
{
    fn from(value: Vec<T>) -> Self {
        Self(value)
    }
}

impl<T> Default for Variants<T>
where
    T: PartialEq,
{
    fn default() -> Self {
        Self(Vec::new())
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
        VariantsConfig::<String> { values: vec!["v".to_string()].into(), ..Default::default()})
    ]
    #[case::values_only(
        r#"{"ac_config_field": "some_variants"}"#,
        VariantsConfig::<String> { ac_config_field: Some("some_variants".to_string()), ..Default::default()})
    ]
    #[case::all(
        r#"{"ac_config_field": "some_variants", "values": ["v1", "v2"]}"#,
        VariantsConfig::<String> { ac_config_field: Some("some_variants".to_string()), values: vec!["v1".to_string(), "v2".to_string()].into()})
    ]
    fn test_variants_config_deserialization(
        #[case] input: &str,
        #[case] expected: VariantsConfig<String>,
    ) {
        let value: VariantsConfig<String> = serde_yaml::from_str(input).unwrap();
        assert_eq!(value, expected);
    }
}
