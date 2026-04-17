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
    #[serde(rename = "string")]
    String(StringFieldsDefinition),
    #[serde(rename = "bool")]
    Bool(FieldsDefinition<bool>),
    #[serde(rename = "number")]
    Number(FieldsDefinition<serde_yaml::Number>),
    #[serde(rename = "map[string]yaml")]
    MapStringYaml(FieldsDefinition<HashMap<String, serde_yaml::Value>>),
    #[serde(rename = "yaml")]
    Yaml(YamlFieldsDefinition),
}

/// [VariableTypeDefinition] including information known at runtime.
#[derive(Debug, PartialEq, Clone)]
pub enum VariableType {
    String(StringFields),
    Bool(Fields<bool>),
    Number(Fields<serde_yaml::Number>),
    MapStringYaml(Fields<HashMap<String, serde_yaml::Value>>),
    Yaml(Fields<serde_yaml::Value>),
}

impl VariableTypeDefinition {
    /// Returns the corresponding [VariableType] according to the provided configuration.
    pub fn with_config(self, constraints: &VariableConstraints) -> VariableType {
        match self {
            VariableTypeDefinition::String(v) => VariableType::String(v.with_config(constraints)),
            VariableTypeDefinition::Bool(v) => VariableType::Bool(v.with_config(constraints)),
            VariableTypeDefinition::Number(v) => VariableType::Number(v.with_config(constraints)),
            VariableTypeDefinition::MapStringYaml(v) => {
                VariableType::MapStringYaml(v.with_config(constraints))
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
            VariableType::MapStringYaml(f) => f.required,
            VariableType::Yaml(f) => f.required,
        }
    }

    pub(crate) fn merge_with_yaml_value(
        &mut self,
        value: serde_yaml::Value,
    ) -> Result<(), AgentTypeError> {
        match self {
            VariableType::String(f) => f.set_final_value(serde_yaml::from_value(value)?),
            VariableType::Bool(f) => f.set_final_value(serde_yaml::from_value(value)?),
            VariableType::Number(f) => f.set_final_value(serde_yaml::from_value(value)?),
            VariableType::MapStringYaml(f) => f.set_final_value(serde_yaml::from_value(value)?),
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
            VariableType::MapStringYaml(f) => f
                .final_value
                .as_ref()
                .or(f.default.as_ref())
                .cloned()
                .map(TrivialValue::MapStringYaml),
            VariableType::Yaml(f) => f
                .final_value
                .as_ref()
                .or(f.default.as_ref())
                .cloned()
                .map(TrivialValue::Yaml),
        }
    }

    pub(crate) fn template_default(
        &mut self,
        global_defaults_vars: &HashMap<String, super::Variable>,
    ) -> Result<(), AgentTypeError> {
        use crate::agent_type::templates::Templateable;

        match self {
            VariableType::String(f) => {
                if let Some(default) = &f.inner.default {
                    // Template the default value
                    let templated = default.clone().template_with(global_defaults_vars)?;
                    f.inner.default = Some(templated);
                }
            }
            VariableType::Bool(f) => {
                // For bool types with template defaults, extract and inject the value directly
                if let Some(template) = &f.default_template {
                    let resolved = template.clone().template_with(global_defaults_vars)?;
                    // Parse the resolved string as bool
                    f.default = Some(resolved.parse().map_err(|_| {
                        AgentTypeError::Parse(format!(
                            "Cannot parse '{}' as bool from template '{}'",
                            resolved, template
                        ))
                    })?);
                }
            }
            VariableType::Number(f) => {
                // For number types with template defaults, extract and inject the value directly
                if let Some(template) = &f.default_template {
                    let resolved = template.clone().template_with(global_defaults_vars)?;
                    // Parse the resolved string as number
                    f.default = Some(serde_yaml::from_str(&resolved).map_err(|e| {
                        AgentTypeError::Parse(format!(
                            "Cannot parse '{}' as number from template '{}': {}",
                            resolved, template, e
                        ))
                    })?);
                }
            }
            VariableType::MapStringYaml(f) => {
                // For map types with template defaults, extract and inject the value directly
                if let Some(template) = &f.default_template {
                    let resolved = template.clone().template_with(global_defaults_vars)?;
                    // Parse the resolved string as map
                    f.default = Some(serde_yaml::from_str(&resolved).map_err(|e| {
                        AgentTypeError::Parse(format!(
                            "Cannot parse '{}' as map from template '{}': {}",
                            resolved, template, e
                        ))
                    })?);
                }
            }
            VariableType::Yaml(f) => {
                // For yaml types with template defaults, extract and inject the value directly
                if let Some(template) = &f.default_template {
                    let resolved = template.clone().template_with(global_defaults_vars)?;
                    // Parse the resolved string as yaml
                    f.default = Some(serde_yaml::from_str(&resolved).map_err(|e| {
                        AgentTypeError::Parse(format!(
                            "Cannot parse '{}' as yaml from template '{}': {}",
                            resolved, template, e
                        ))
                    })?);
                }
            }
        }
        Ok(())
    }
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

    impl From<Fields<serde_yaml::Number>> for VariableType {
        fn from(fields: Fields<serde_yaml::Number>) -> Self {
            VariableType::Number(fields)
        }
    }

    impl From<Fields<HashMap<String, serde_yaml::Value>>> for VariableType {
        fn from(fields: Fields<HashMap<String, serde_yaml::Value>>) -> Self {
            VariableType::MapStringYaml(fields)
        }
    }

    impl From<Fields<serde_yaml::Value>> for VariableType {
        fn from(fields: Fields<serde_yaml::Value>) -> Self {
            VariableType::Yaml(fields)
        }
    }
}
