use std::{
    collections::HashMap as Map,
    fmt::{Display, Formatter},
};

use crate::config::agent_type::agent_types::AgentTypeEndSpec;
use serde::{Deserialize, Serialize};

use super::{agent_types::VariableType, error::AgentTypeError};

/// Represents all the allowed types for a configuration defined in the spec value.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TrivialValue {
    String(String),
    #[serde(skip)]
    File(FilePathWithContent),
    #[serde(skip)]
    Yaml(YamlValue),
    Bool(bool),
    Number(N),
    Map(Map<String, TrivialValue>),
}

impl TrivialValue {
    /// Checks the `TrivialValue` against the given [`VariableType`], erroring if they do not match.
    ///
    /// This is also in charge of converting a `TrivialValue::String` into a `TrivialValue::File`, using the actual string as the file content, if the given [`VariableType`] is `VariableType::File`.
    pub fn check_type<T>(self, end_spec: &T) -> Result<Self, AgentTypeError>
    where
        T: AgentTypeEndSpec,
    {
        match (self.clone(), end_spec.variable_type()) {
            (TrivialValue::String(_), VariableType::String)
            | (TrivialValue::Bool(_), VariableType::Bool)
            | (TrivialValue::File(_), VariableType::File)
            | (TrivialValue::Yaml(_), VariableType::Yaml)
            | (TrivialValue::Number(_), VariableType::Number) => Ok(self),
            (TrivialValue::Map(m), VariableType::MapStringString) => {
                if !m.iter().all(|(_, v)| matches!(v, TrivialValue::String(_))) {
                    return Err(AgentTypeError::InvalidMap);
                }
                Ok(self)
            }
            (TrivialValue::Map(m), VariableType::MapStringFile) => {
                if !m.iter().all(|(_, v)| matches!(v, TrivialValue::String(_))) {
                    return Err(AgentTypeError::InvalidMap);
                }

                if end_spec.file_path().is_none() {
                    return Err(AgentTypeError::InvalidFilePath);
                }

                Ok(TrivialValue::Map(
                    m.into_iter()
                        .map(|(k, v)| {
                            (
                                k,
                                // it's safe to make unwrap() as we previously checked is not none
                                TrivialValue::File(FilePathWithContent::new(
                                    end_spec.file_path().unwrap(),
                                    v.to_string(),
                                )),
                            )
                        })
                        .collect(),
                ))
            }
            (TrivialValue::String(content), VariableType::File) => match end_spec.file_path() {
                None => Err(AgentTypeError::InvalidFilePath),
                Some(file_path) => Ok(TrivialValue::File(FilePathWithContent::new(
                    file_path, content,
                ))),
            },
            (TrivialValue::String(content), VariableType::Yaml) => {
                let yaml_value: YamlValue = content.try_into()?;
                Ok(TrivialValue::Yaml(yaml_value))
            }
            (v, t) => Err(AgentTypeError::TypeMismatch {
                expected_type: t,
                actual_value: v,
            }),
        }
    }

    /// If the trivial value is a yaml, it returns a copy the corresponding [serde_yaml::Value], returns None otherwise.
    pub fn to_yaml_value(&self) -> Option<serde_yaml::Value> {
        match self {
            Self::Yaml(yaml) => Some(yaml.value.clone()),
            _ => None,
        }
    }
}

impl Display for TrivialValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TrivialValue::String(s) => write!(f, "{}", s),
            TrivialValue::File(file) => write!(f, "{}", file.path),
            TrivialValue::Yaml(yaml) => write!(f, "{}", yaml.content),
            TrivialValue::Bool(b) => write!(f, "{}", b),
            TrivialValue::Number(n) => write!(f, "{}", n),
            TrivialValue::Map(n) => {
                let flatten: Vec<String> = n
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect();
                write!(f, "{}", flatten.join(" "))
            }
        }
    }
}

/// Represents a file path and its content.
#[derive(Debug, PartialEq, Default, Clone, Deserialize)]
pub struct FilePathWithContent {
    #[serde(skip)]
    pub path: String,
    #[serde(flatten)]
    pub content: String,
}

impl FilePathWithContent {
    pub fn new(path: String, content: String) -> Self {
        FilePathWithContent { path, content }
    }
}

/// Represents a numeric value, which can be either a positive integer, a negative integer or a float.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum N {
    PosInt(u64),
    /// Always less than zero.
    NegInt(i64),
    /// May be infinite or NaN.
    Float(f64),
}

impl Display for N {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            N::PosInt(n) => write!(f, "{}", n),
            N::NegInt(n) => write!(f, "{}", n),
            N::Float(n) => write!(f, "{}", n),
        }
    }
}

/// Represents a yaml value, holding both the string before deserializing and the [serde_yaml::Value] after.
#[derive(Debug, PartialEq, Default, Clone, Deserialize)]
pub struct YamlValue {
    #[serde(skip)]
    pub value: serde_yaml::Value,
    #[serde(flatten)]
    pub content: String,
}

impl TryFrom<String> for YamlValue {
    type Error = serde_yaml::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(Self {
            value: serde_yaml::from_str(value.as_str())?,
            content: value,
        })
    }
}
