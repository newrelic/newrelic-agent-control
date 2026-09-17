//! This module defines the Agent Type variables, including their serialized representation and
//! the corresponding functionality.
//!
//! A [`VariableDefinition`] is the static shape parsed from an Agent Type YAML. It is resolved
//! against user-supplied values via [`tree::VariableTree::resolve`] to produce the resolved
//! [`VariableValue`].

pub mod dynamic_variables;
pub mod name;
pub mod namespace;
pub mod tree;
pub mod value;

use crate::agent_type::{
    error::AgentTypeError,
    variable::value::{VariableType, VariableValue},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Static Variable definition defines the supported fields for a variable in an Agent Type.
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct VariableDefinition {
    pub(crate) default: Option<VariableValue>,
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
            #[serde(flatten)]
            variable_type: VariableType,
        }

        let raw = Raw::deserialize(deserializer)?;

        let default = normalize_default(raw.default, raw.required, &raw.variable_type)
            .map_err(serde::de::Error::custom)?;

        Ok(VariableDefinition {
            default,
            variable_type: raw.variable_type,
        })
    }
}

/// Validates the `required`/`default` combination and fills the implicit YAML `null` default for
/// optional `yaml` variables that omit one.
fn normalize_default(
    raw_default: Option<serde_json::Value>,
    required: bool,
    variable_type: &VariableType,
) -> Result<Option<VariableValue>, AgentTypeError> {
    let coerced_default = raw_default
        .map(|d| coerce_serde_value(variable_type, d))
        .transpose()?;

    match (required, coerced_default) {
        (true, None) => Ok(None),
        (false, Some(v)) => Ok(Some(v)),
        (true, Some(_)) => Err(AgentTypeError::Parse(
            "default value cannot be specified for a required spec key".to_string(),
        )),
        (false, None) => {
            if matches!(variable_type, VariableType::Yaml) {
                return Ok(Some(VariableValue::Yaml(serde_json::Value::Null)));
            }
            Err(AgentTypeError::Parse(
                "missing default value for a non-required spec key".to_string(),
            ))
        }
    }
}

impl VariableDefinition {
    /// Returns the variable's declared type.
    pub fn kind(&self) -> &VariableType {
        &self.variable_type
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
    use crate::agent_type::variable::value::VariableType;
    use crate::agent_type::{
        variable::value::VariableValue,
        variable::{VariableDefinition, tree::VariableTreeNode},
    };
    use rstest::rstest;
    use std::collections::HashMap;

    #[test]
    fn variable_definition_kind_returns_declared_type() {
        let variable_type = VariableType::String;
        let definition = VariableDefinition {
            default: None,
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

    #[rstest]
    fn variable_definition_ignores_legacy_variants_field() {
        // Old agent type YAMLs may still carry a `variants:` block. Parsing must accept and
        // silently drop it so legacy definitions keep loading after the feature was removed.
        let value = r#"
type: string
required: true
variants:
  values: ["a", "b"]
"#
        .to_string();
        assert!(serde_saphyr::from_str::<VariableDefinition>(&value).is_ok());
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
                        variable_type: VariableType::String,
                    }),
                )])),
            )])),
        )]));
        assert_eq!(tree, expected);
    }
}
