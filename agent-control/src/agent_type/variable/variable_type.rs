//! This module defines the supported types for Agent Type variables.

use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent_type::{
    error::AgentTypeError,
    trivial_value::{FilePathWithContent, TrivialValue},
};

use super::fields::{Fields, FieldsDefinition, FieldsWithPath, FieldsWithPathDefinition};

/// Defines the supported values for the `type` field in AgentTypes, each variant also defines the
/// rest of the fields that are supported for variables of that type.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum VariableTypeDefinition {
    #[serde(rename = "string")]
    String(FieldsDefinition<String>),
    #[serde(rename = "bool")]
    Bool(FieldsDefinition<bool>),
    #[serde(rename = "number")]
    Number(FieldsDefinition<serde_yaml::Number>),
    #[serde(rename = "file")]
    File(FieldsWithPathDefinition<FilePathWithContent>),
    #[serde(rename = "map[string]string")]
    MapStringString(FieldsDefinition<HashMap<String, String>>),
    #[serde(rename = "map[string]file")]
    MapStringFile(FieldsWithPathDefinition<HashMap<String, FilePathWithContent>>),
    #[serde(rename = "yaml")]
    Yaml(FieldsDefinition<serde_yaml::Value>),
}

/// [VariableTypeDefinition] including information known at runtime.
#[derive(Debug, PartialEq, Clone)]
pub enum VariableType {
    String(Fields<String>),
    Bool(Fields<bool>),
    Number(Fields<serde_yaml::Number>),
    File(FieldsWithPath<FilePathWithContent>),
    MapStringString(Fields<HashMap<String, String>>),
    MapStringFile(FieldsWithPath<HashMap<String, FilePathWithContent>>),
    Yaml(Fields<serde_yaml::Value>),
}

impl VariableTypeDefinition {
    /// Returns the corresponding [VariableType] according to the provided configuration.
    // TODO: actually receive configuration
    pub fn with_config(self) -> VariableType {
        match self {
            VariableTypeDefinition::String(v) => VariableType::String(v.with_config()),
            VariableTypeDefinition::Bool(v) => VariableType::Bool(v.with_config()),
            VariableTypeDefinition::Number(v) => VariableType::Number(v.with_config()),
            VariableTypeDefinition::File(v) => VariableType::File(v.with_config()),
            VariableTypeDefinition::MapStringString(v) => {
                VariableType::MapStringString(v.with_config())
            }
            VariableTypeDefinition::MapStringFile(v) => {
                VariableType::MapStringFile(v.with_config())
            }
            VariableTypeDefinition::Yaml(v) => VariableType::Yaml(v.with_config()),
        }
    }
}

impl From<Fields<String>> for VariableType {
    fn from(fields: Fields<String>) -> Self {
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

impl From<FieldsWithPath<FilePathWithContent>> for VariableType {
    fn from(fields: FieldsWithPath<FilePathWithContent>) -> Self {
        VariableType::File(fields)
    }
}

impl From<Fields<HashMap<String, String>>> for VariableType {
    fn from(fields: Fields<HashMap<String, String>>) -> Self {
        VariableType::MapStringString(fields)
    }
}

impl From<FieldsWithPath<HashMap<String, FilePathWithContent>>> for VariableType {
    fn from(fields: FieldsWithPath<HashMap<String, FilePathWithContent>>) -> Self {
        VariableType::MapStringFile(fields)
    }
}

impl From<Fields<serde_yaml::Value>> for VariableType {
    fn from(fields: Fields<serde_yaml::Value>) -> Self {
        VariableType::Yaml(fields)
    }
}

/// The below methods are mostly concerned with delegating to the inner type on each `Kind` variant.
/// It's a lot of boilerplate, but declarative and straight-forward.
impl VariableType {
    pub(crate) fn is_required(&self) -> bool {
        match self {
            VariableType::String(f) => f.required,
            VariableType::Bool(f) => f.required,
            VariableType::Number(f) => f.required,
            VariableType::File(f) => f.inner.required,
            VariableType::MapStringString(f) => f.required,
            VariableType::MapStringFile(f) => f.inner.required,
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
            VariableType::File(f) => {
                let mut file: FilePathWithContent = serde_yaml::from_value(value)?;
                file.with_path(f.file_path.clone());
                f.inner.set_final_value(file)
            }
            VariableType::MapStringString(f) => f.set_final_value(serde_yaml::from_value(value)?),
            VariableType::MapStringFile(f) => {
                let mut files: HashMap<String, FilePathWithContent> =
                    serde_yaml::from_value(value)?;
                files
                    .values_mut()
                    .for_each(|fp| fp.with_path(f.file_path.clone()));
                f.inner.set_final_value(files)
            }
            VariableType::Yaml(f) => f.set_final_value(value),
        }?;
        Ok(())
    }

    pub(crate) fn get_final_value(&self) -> Option<TrivialValue> {
        match self {
            VariableType::String(f) => f
                .final_value
                .as_ref()
                .or(f.default.as_ref())
                .cloned()
                .map(TrivialValue::String),
            VariableType::Bool(f) => f.final_value.or(f.default).map(TrivialValue::Bool),
            VariableType::Number(f) => f
                .final_value
                .as_ref()
                .or(f.default.as_ref())
                .cloned()
                .map(TrivialValue::Number),
            VariableType::File(f) => f
                .inner
                .final_value
                .as_ref()
                .or({
                    let mut file = f.inner.default.clone();
                    if let Some(fp) = file.as_mut() {
                        fp.with_path(f.file_path.clone())
                    }
                    file
                }
                .as_ref())
                .cloned()
                .map(TrivialValue::File),
            VariableType::MapStringString(f) => f
                .final_value
                .as_ref()
                .or(f.default.as_ref())
                .cloned()
                .map(TrivialValue::MapStringString),
            VariableType::MapStringFile(f) => f
                .inner
                .final_value
                .as_ref()
                .or(f.inner.default.as_ref())
                .cloned()
                .map(TrivialValue::MapStringFile),
            VariableType::Yaml(f) => f
                .final_value
                .as_ref()
                .or(f.default.as_ref())
                .cloned()
                .map(TrivialValue::Yaml),
        }
    }

    pub(crate) fn get_file_path(&self) -> Option<&PathBuf> {
        match self {
            VariableType::File(f) => Some(f.get_file_path()),
            VariableType::MapStringFile(f) => Some(f.get_file_path()),
            _ => None,
        }
    }

    pub(crate) fn set_file_path(&mut self, file_path: PathBuf) {
        match self {
            VariableType::File(f) => f.set_file_path(file_path),
            VariableType::MapStringFile(f) => f.set_file_path(file_path),
            _ => {}
        }
    }
}
