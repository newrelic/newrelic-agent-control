//! This module defines the type to configure variants which can restrict Agent Type values to a particular
//! collection of supported values.

use serde::{Deserialize, Serialize};

/// Represents a collection of supported variants for a variable.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Variants<T: PartialEq>(pub(crate) Vec<T>); // TODO: we may not need it to be public

/// Defines the configuration to be set when defining [Variants] from Agent Control configuration.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct VariantsConfig<T>
where
    T: PartialEq,
{
    pub(crate) ac_config_field: String,
    pub(crate) default: Variants<T>,
}

/// Defines the supported variants definition.
/// # Examples:
///
/// ```
/// # use newrelic_agent_control::agent_type::variable::variants::VariantsDefinition;
/// # use assert_matches::assert_matches;
/// // Variants defined in Agent Type
/// let s = r#"["value1", "value2"]"#;
/// let v: VariantsDefinition<String> = serde_yaml::from_str(s).unwrap();
/// assert_matches!(v, VariantsDefinition::<String>::FromAgentType(_));
///
/// // Variants defined as Agent Control configuration reference
/// let s = r#"{"ac_config_field": "some_field_name", "default": ["value1"]}"#;
/// let v: VariantsDefinition<String> = serde_yaml::from_str(s).unwrap();
/// assert_matches!(v, VariantsDefinition::<String>::FromAgentControlConfig(_));
/// ```
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VariantsDefinition<T>
where
    T: PartialEq,
{
    /// Variants directly set in the Agent Type definition.
    FromAgentType(Variants<T>),
    /// Variants set in Agent Control static configuration
    FromAgentControlConfig(VariantsConfig<T>),
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
