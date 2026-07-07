//! Agent-Type variable definition and its runtime counterpart.
//!
//! Variables are untyped: the user may supply any YAML value. The renderer stringifies by default;
//! the `| toYAML` pipe (see `templates.rs`) opts into raw YAML substitution.

pub mod constraints;
pub mod namespace;
pub mod secret_variables;
pub mod tree;
pub mod variants;

use serde::{Deserialize, Deserializer, Serialize};

use crate::agent_type::{
    error::AgentTypeError,
    variable::{
        constraints::{VariableConstraints, VariantsConstraints},
        variants::{Variants, VariantsConfig},
    },
};

/// Static Variable definition — the shape deserialized from Agent Type YAML.
///
/// Variables are untyped: `default` may hold any YAML value, including explicit `null`. For
/// backward compatibility with agent types authored under the typed model, when a variable is
/// `required: false` and no `default:` key is present, its default becomes `Value::Null` — so the
/// variable has a value ready even when the user does not supply one.
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct VariableDefinition {
    pub(crate) description: String,
    pub(crate) required: bool,
    pub(crate) default: Option<serde_json::Value>,
    pub(crate) variants: VariantsConfig,
}

impl<'de> Deserialize<'de> for VariableDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // We first deserialize into a generic `serde_json::Value` so we can distinguish an
        // absent `default:` key from a present `default: null`.
        let value = serde_json::Value::deserialize(deserializer)?;
        let mut map = match value {
            serde_json::Value::Object(map) => map,
            _ => {
                return Err(serde::de::Error::custom(
                    "expected a mapping for variable definition",
                ));
            }
        };

        let description: String = map
            .remove("description")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .ok_or_else(|| serde::de::Error::missing_field("description"))?;

        let required: bool = map
            .remove("required")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or(false);

        let default = match map.remove("default") {
            // Present with any value (including explicit `null` -> `Value::Null`).
            Some(v) => Some(v),
            // Absent: for non-required variables substitute `Value::Null` so a value is always
            // ready; for required variables leave it as `None`.
            None if !required => Some(serde_json::Value::Null),
            None => None,
        };

        let variants: VariantsConfig = map
            .remove("variants")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();

        // Legacy `type:` and any other unknown keys are silently ignored.

        Ok(VariableDefinition {
            description,
            required,
            default,
            variants,
        })
    }
}

/// [VariableDefinition] including information known at runtime.
#[derive(Debug, PartialEq, Clone)]
pub struct Variable {
    pub(crate) description: String,
    pub(crate) required: bool,
    pub(crate) default: Option<serde_json::Value>,
    pub(crate) final_value: Option<serde_json::Value>,
    pub(crate) variants: Variants,
}

impl VariableDefinition {
    /// Returns the corresponding [Variable] according to the provided configuration.
    pub fn with_config(self, constraints: &VariableConstraints) -> Variable {
        let variants = build_variants(self.variants, &constraints.variants);
        Variable {
            description: self.description,
            required: self.required,
            default: self.default,
            final_value: None,
            variants,
        }
    }
}

fn build_variants(config: VariantsConfig, constraints: &VariantsConstraints) -> Variants {
    let Some(ac_config_field) = config.ac_config_field.as_ref() else {
        return config.values;
    };
    let Some(supported_values) = constraints.get(ac_config_field) else {
        tracing::debug!(
            %ac_config_field,
            "The variants pointed in Agent Type are not set in Agent Control configuration, using defaults"
        );
        return config.values;
    };
    supported_values.into()
}

impl Variable {
    /// Builds a string variable already populated with its final value.
    pub fn new_final_string_variable(final_value: impl ToString) -> Self {
        Self {
            description: String::new(),
            required: false,
            default: None,
            final_value: Some(serde_json::Value::String(final_value.to_string())),
            variants: Variants::default(),
        }
    }

    /// Returns whether this variable must be provided with a value.
    pub fn is_required(&self) -> bool {
        self.required
    }

    /// Returns the variable's final value (its set value, or its default), if any.
    pub fn get_final_value(&self) -> Option<serde_json::Value> {
        self.final_value.clone().or_else(|| self.default.clone())
    }

    /// Sets the variable's final value from the given YAML value, checking variants if any.
    pub fn merge_with_yaml_value(
        &mut self,
        yaml: serde_json::Value,
    ) -> Result<(), AgentTypeError> {
        if !self.variants.is_valid(&yaml) {
            return Err(AgentTypeError::InvalidVariant(self.variants.to_string()));
        }
        self.final_value = Some(yaml);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::agent_type::variable::tree::Tree;
    use crate::agent_type::variable::variants::{Variants, VariantsConfig};

    impl Variable {
        pub(crate) fn new<T>(
            description: String,
            required: bool,
            default: Option<T>,
            final_value: Option<T>,
        ) -> Self
        where
            T: Into<serde_json::Value>,
        {
            Self {
                description,
                required,
                default: default.map(Into::into),
                final_value: final_value.map(Into::into),
                variants: Variants::default(),
            }
        }

        pub(crate) fn new_string(
            description: String,
            required: bool,
            default: Option<String>,
            final_value: Option<String>,
        ) -> Self {
            Self {
                description,
                required,
                default: default.map(serde_json::Value::String),
                final_value: final_value.map(serde_json::Value::String),
                variants: Variants::default(),
            }
        }
    }

    #[test]
    fn variable_definition_tree_deserialize() {
        let value = r#"
foo:
  bar:
    var_name:
      description: "some description"
      required: false
      default: "a"
      variants:
        ac_config_field: "foo.bar.var_name"
        values: ["a", "b"]
"#;
        let tree: Tree<VariableDefinition> = serde_saphyr::from_str(value).unwrap();
        let expected: Tree<VariableDefinition> = Tree::Mapping(HashMap::from([(
            "foo".to_string(),
            Tree::Mapping(HashMap::from([(
                "bar".to_string(),
                Tree::Mapping(HashMap::from([(
                    "var_name".to_string(),
                    Tree::End(VariableDefinition {
                        description: "some description".to_string(),
                        required: false,
                        default: Some(serde_json::Value::String("a".into())),
                        variants: VariantsConfig {
                            ac_config_field: Some("foo.bar.var_name".to_string()),
                            values: vec!["a".to_string(), "b".to_string()].into(),
                        },
                    }),
                )])),
            )])),
        )]));
        assert_eq!(tree, expected);
    }

    #[test]
    fn variable_definition_ignores_legacy_type_field() {
        let yaml = r#"
description: "legacy"
type: yaml
required: false
default: {}
"#;
        let def: VariableDefinition = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(def.description, "legacy");
        assert_eq!(def.default, Some(serde_json::json!({})));
    }
}
