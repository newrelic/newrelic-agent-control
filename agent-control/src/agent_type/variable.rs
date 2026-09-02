//! This module defines the Agent Type variables, including their serialized representation and
//! the corresponding functionality.
//!
//! A [`VariableDefinition`] is the static shape parsed from an Agent Type YAML. Once we have the
//! AC-wide constraints and the user-supplied values, [`VariableDefinition::resolve_variable_value`] produces
//! the resolved [`VariableValue`] directly — there is no intermediate "runtime variable" wrapper.

pub mod constraints;
pub mod dynamic_variables;
pub mod name;
pub mod namespace;
pub mod tree;
pub mod variants;

use crate::agent_type::variable::variants::Variants;
use crate::agent_type::{
    error::AgentTypeError,
    variable::{constraints::VariableConstraints, variants::VariantsConfig},
    variable_value::{VariableType, VariableValue},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Static Variable definition defines the supported fields for a variable in an Agent Type.
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct VariableDefinition {
    pub(crate) default: Option<VariableValue>,
    /// Allowed values for `string`-typed variables. `None` for other types.
    pub(crate) variants: Option<VariantsConfig>,
    #[serde(flatten)]
    pub(crate) variable_type: VariableType,
}

impl<'de> Deserialize<'de> for VariableDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            required: bool,
            #[serde(default)]
            default: Option<serde_json::Value>,
            #[serde(default)]
            variants: Option<VariantsConfig>,
            #[serde(flatten)]
            variable_type: VariableType,
        }

        let raw = Raw::deserialize(deserializer)?;

        let default = raw
            .default
            .map(|d| coerce_serde_value(&raw.variable_type, d))
            .transpose()
            .map_err(serde::de::Error::custom)?;
        let mut def = VariableDefinition {
            default,
            variants: raw.variants,
            variable_type: raw.variable_type,
        };
        def.validate_and_normalize(raw.required)
            .map_err(serde::de::Error::custom)?;
        Ok(def)
    }
}

impl VariableDefinition {
    /// Returns the variable's declared type.
    pub fn kind(&self) -> &VariableType {
        &self.variable_type
    }

    /// Validates the cross-field invariants between `required`, `default` and `variants`, and
    /// fills the implicit YAML `null` default for optional `yaml` variables that omit one.
    fn validate_and_normalize(&mut self, required: bool) -> Result<(), AgentTypeError> {
        if self.variants.is_some() && !matches!(self.variable_type, VariableType::String) {
            return Err(AgentTypeError::Parse(
                "`variants` is only supported for `string`-typed variables".to_string(),
            ));
        }
        let has_default = self.default.is_some();
        if required && has_default {
            return Err(AgentTypeError::Parse(
                "default value cannot be specified for a required spec key".to_string(),
            ));
        }
        if !required && !has_default {
            if matches!(self.variable_type, VariableType::Yaml) {
                self.default = Some(VariableValue::Yaml(serde_json::Value::Null));
            } else {
                return Err(AgentTypeError::Parse(
                    "missing default value for a non-required spec key".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Resolves this definition into a fully-populated [`VariableValue`] using the given AC
    /// constraints and an optional user-supplied value. Errors when the user value doesn't match
    /// the declared type/variants, or when the variable is required and no value was provided.
    pub fn resolve_variable_value(
        self,
        constraints: &VariableConstraints,
        user_value: Option<serde_json::Value>,
    ) -> Result<VariableValue, AgentTypeError> {
        match user_value {
            Some(v) => {
                let coerced = coerce_serde_value(&self.variable_type, v)?;
                if let (Some(cfg), VariableValue::String(s)) = (&self.variants, &coerced) {
                    let resolved = Variants::new(cfg, &constraints.variants);
                    if !resolved.is_valid(s) {
                        return Err(AgentTypeError::InvalidVariant(resolved.to_string()));
                    }
                }
                Ok(coerced)
            }
            None => self
                .default
                .ok_or_else(|| AgentTypeError::ValuesNotPopulated(Vec::new())),
        }
    }
}

/// Coerces a YAML value at resolve time. For `string_map`, non-string map values
/// are accepted and encoded as their YAML text form via [`parse_string_map`].
fn coerce_serde_value(
    variable_type: &VariableType,
    value: serde_json::Value,
) -> Result<VariableValue, AgentTypeError> {
    let coerced = match variable_type {
        VariableType::String => VariableValue::String(serde_json::from_value(value)?),
        VariableType::Bool => VariableValue::Bool(serde_json::from_value(value)?),
        VariableType::Number => VariableValue::Number(serde_json::from_value(value)?),
        VariableType::StringMap => VariableValue::MapStringString(parse_string_map(value)?),
        VariableType::Yaml => VariableValue::Yaml(value),
    };
    Ok(coerced)
}

/// Converts a JSON value into a `HashMap<String, String>`, encoding non-string map values as their
/// YAML text form. Used when merging a config value into a `string_map`-typed variable.
pub(super) fn parse_string_map(
    value: serde_json::Value,
) -> Result<HashMap<String, String>, AgentTypeError> {
    let map: HashMap<String, serde_json::Value> = serde_json::from_value(value)?;
    map.into_iter()
        .map(|(key, value)| {
            let value = match value {
                serde_json::Value::String(s) => s,
                other => serde_saphyr::to_string(&other).map_err(|e| {
                    AgentTypeError::Parse(format!(
                        "could not encode string_map value for '{key}': {e}"
                    ))
                })?,
            };
            Ok((key, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::agent_type::variable_value::VariableType;
    use crate::agent_type::{
        variable::{VariableDefinition, tree::VariableTreeNode, variants::VariantsConfig},
        variable_value::VariableValue,
    };
    use std::collections::HashMap;

    #[test]
    fn variable_definition_kind_returns_declared_type() {
        let variable_type = VariableType::String;
        let definition = VariableDefinition {
            default: None,
            variants: None,
            variable_type: variable_type.clone(),
        };

        assert_eq!(definition.kind(), &variable_type);
    }

    #[test]
    fn variable_definition_required_with_default_is_rejected() {
        let value = r#"
type: string
required: true
default: "a"
"#;
        let err = serde_saphyr::from_str::<VariableDefinition>(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("default value cannot be specified for a required spec key"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn variable_definition_non_required_without_default_is_rejected() {
        let value = r#"
type: string
required: false
"#;
        let err = serde_saphyr::from_str::<VariableDefinition>(value).unwrap_err();
        assert!(
            err.to_string()
                .contains("missing default value for a non-required spec key"),
            "unexpected error: {err}",
        );
    }

    #[test]
    fn variable_definition_yaml_default_defaults_to_null_when_absent() {
        let value = r#"
type: yaml
required: false
"#;
        let def: VariableDefinition = serde_saphyr::from_str(value).unwrap();
        assert!(matches!(def.variable_type, VariableType::Yaml));
        assert_eq!(
            def.default,
            Some(VariableValue::Yaml(serde_json::Value::Null))
        );
    }

    #[test]
    fn variable_definition_yaml_required_without_default_is_accepted() {
        let value = r#"
type: yaml
required: true
"#;
        let def: VariableDefinition = serde_saphyr::from_str(value).unwrap();
        assert!(matches!(def.variable_type, VariableType::Yaml));
        assert_eq!(def.default, None);
    }

    #[test]
    fn variable_definition_tree_deserialize() {
        let value = r#"
foo:
  bar:
    var_name:
      type: string
      required: false
      default: "a"
      variants:
        ac_config_field: "foo.bar.var_name"
        values: ["a", "b"]
"#;
        let tree: VariableTreeNode = serde_saphyr::from_str(value).unwrap();
        let expected: VariableTreeNode = VariableTreeNode::Mapping(HashMap::from([(
            "foo".to_string(),
            VariableTreeNode::Mapping(HashMap::from([(
                "bar".to_string(),
                VariableTreeNode::Mapping(HashMap::from([(
                    "var_name".to_string(),
                    VariableTreeNode::End(VariableDefinition {
                        default: Some(VariableValue::String("a".to_string())),
                        variants: Some(VariantsConfig {
                            ac_config_field: Some("foo.bar.var_name".to_string()),
                            values: vec!["a".to_string(), "b".to_string()].into(),
                        }),
                        variable_type: VariableType::String,
                    }),
                )])),
            )])),
        )]));
        assert_eq!(tree, expected);
    }
}
