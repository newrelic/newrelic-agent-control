//! This module defines the supported types for Agent Type variables.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::agent_type::{
    error::AgentTypeError,
    trivial_value::TrivialValue,
    variable::{
        constraints::VariableConstraints,
        fields::{StringFields, StringFieldsDefinition, YamlFieldsDefinition},
    },
};

use super::fields::{Fields, FieldsDefinition};

/// Defines the supported values for the `type` field in AgentTypes, each variant also defines the
/// rest of the fields that are supported for variables of that type.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum VariableTypeDefinition {
    /// A string-typed variable.
    #[serde(rename = "string")]
    String(StringFieldsDefinition),
    /// A boolean-typed variable.
    #[serde(rename = "bool")]
    Bool(FieldsDefinition<bool>),
    /// A number-typed variable.
    #[serde(rename = "number")]
    Number(FieldsDefinition<serde_json::Number>),
    /// A  map of string keys to string values.
    /// A merged value that isn't already a string is accepted and encoded as
    /// its YAML text form.
    #[serde(rename = "string_map")]
    StringMap(FieldsDefinition<HashMap<String, String>>),
    /// A yaml-typed variable.
    #[serde(rename = "yaml")]
    Yaml(YamlFieldsDefinition),
}

/// [VariableTypeDefinition] including information known at runtime.
#[derive(Debug, PartialEq, Clone)]
pub enum VariableType {
    /// A string-typed variable.
    String(StringFields),
    /// A boolean-typed variable.
    Bool(Fields<bool>),
    /// A number-typed variable.
    Number(Fields<serde_json::Number>),
    /// A `string_map`-typed variable.
    StringMap(Fields<HashMap<String, String>>),
    /// A yaml-typed variable.
    Yaml(Fields<serde_json::Value>),
}

impl VariableTypeDefinition {
    /// Returns the corresponding [VariableType] according to the provided configuration.
    pub fn with_config(self, constraints: &VariableConstraints) -> VariableType {
        match self {
            VariableTypeDefinition::String(v) => VariableType::String(v.with_config(constraints)),
            VariableTypeDefinition::Bool(v) => VariableType::Bool(v.with_config(constraints)),
            VariableTypeDefinition::Number(v) => VariableType::Number(v.with_config(constraints)),
            VariableTypeDefinition::StringMap(v) => {
                VariableType::StringMap(v.with_config(constraints))
            }
            VariableTypeDefinition::Yaml(v) => VariableType::Yaml(v.with_config(constraints)),
        }
    }
}

/// The below methods are mostly concerned with delegating to the inner type on each `Kind` variant.
/// It's a lot of boilerplate, but declarative and straight-forward.
impl VariableType {
    pub(crate) fn is_required(&self) -> bool {
        match self {
            VariableType::String(f) => f.inner.required,
            VariableType::Bool(f) => f.required,
            VariableType::Number(f) => f.required,
            VariableType::StringMap(f) => f.required,
            VariableType::Yaml(f) => f.required,
        }
    }

    pub(crate) fn merge_with_yaml_value(
        &mut self,
        value: serde_json::Value,
    ) -> Result<(), AgentTypeError> {
        match self {
            VariableType::String(f) => f.set_final_value(serde_json::from_value(value)?),
            VariableType::Bool(f) => f.set_final_value(serde_json::from_value(value)?),
            VariableType::Number(f) => f.set_final_value(serde_json::from_value(value)?),
            VariableType::StringMap(f) => f.set_final_value(parse_string_map(value)?),
            VariableType::Yaml(f) => f.set_final_value(value),
        }?;
        Ok(())
    }

    pub(crate) fn get_final_value(&self) -> Option<TrivialValue> {
        match self {
            VariableType::String(f) => f
                .inner
                .final_value
                .as_ref()
                .or(f.inner.default.as_ref())
                .cloned()
                .map(TrivialValue::String),
            VariableType::Bool(f) => f.final_value.or(f.default).map(TrivialValue::Bool),
            VariableType::Number(f) => f
                .final_value
                .as_ref()
                .or(f.default.as_ref())
                .cloned()
                .map(TrivialValue::Number),
            VariableType::StringMap(f) => f
                .final_value
                .as_ref()
                .or(f.default.as_ref())
                .cloned()
                .map(TrivialValue::MapStringString),
            VariableType::Yaml(f) => f
                .final_value
                .as_ref()
                .or(f.default.as_ref())
                .cloned()
                .map(TrivialValue::Yaml),
        }
    }
}

fn parse_string_map(value: serde_json::Value) -> Result<HashMap<String, String>, AgentTypeError> {
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
    use super::*;

    impl From<StringFields> for VariableType {
        fn from(fields: StringFields) -> Self {
            VariableType::String(fields)
        }
    }

    impl From<Fields<bool>> for VariableType {
        fn from(fields: Fields<bool>) -> Self {
            VariableType::Bool(fields)
        }
    }

    impl From<Fields<serde_json::Number>> for VariableType {
        fn from(fields: Fields<serde_json::Number>) -> Self {
            VariableType::Number(fields)
        }
    }

    impl From<Fields<HashMap<String, String>>> for VariableType {
        fn from(fields: Fields<HashMap<String, String>>) -> Self {
            VariableType::StringMap(fields)
        }
    }

    impl From<Fields<serde_json::Value>> for VariableType {
        fn from(fields: Fields<serde_json::Value>) -> Self {
            VariableType::Yaml(fields)
        }
    }

    fn empty_string_map() -> VariableType {
        VariableType::StringMap(Fields {
            required: false,
            default: Some(HashMap::new()),
            final_value: None,
        })
    }

    #[test]
    fn string_map_merge_accepts_plain_string_value() {
        let mut variable_type = empty_string_map();

        variable_type
            .merge_with_yaml_value(serde_json::json!({ "file.txt": "hello" }))
            .unwrap();

        let Some(TrivialValue::MapStringString(map)) = variable_type.get_final_value() else {
            panic!("expected a MapStringString value");
        };
        assert_eq!(map.get("file.txt"), Some(&"hello".to_string()));
    }

    #[test]
    fn string_map_merge_accepts_other_values() {
        let mut variable_type = empty_string_map();

        let nested = serde_json::json!({"logs": [{"name": "syslog"}]});
        variable_type
            .merge_with_yaml_value(serde_json::json!({ "logging.yml": nested.clone() }))
            .unwrap();

        let expected_content = serde_saphyr::to_string(&nested).unwrap();
        let Some(TrivialValue::MapStringString(map)) = variable_type.get_final_value() else {
            panic!("expected a MapStringString value");
        };
        assert_eq!(map.get("logging.yml"), Some(&expected_content));
    }
}
