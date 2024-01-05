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
    Yaml(serde_yaml::Value),
    Bool(bool),
    Number(Number),
    #[serde(skip)]
    MapStringString(Map<String, String>),
    #[serde(skip)]
    MapStringFile(Map<String, FilePathWithContent>),
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
            | (TrivialValue::Number(_), VariableType::Number)
            | (TrivialValue::Yaml(_), VariableType::Yaml)
            | (TrivialValue::MapStringString(_), VariableType::MapStringString)
            | (TrivialValue::MapStringFile(_), VariableType::MapStringFile) => Ok(self),
            (v, t) => Err(AgentTypeError::TypeMismatch {
                expected_type: t,
                actual_value: v,
            }),
        }
    }

    /// If the trivial value is a yaml, it returns a copy the corresponding [serde_yaml::Value], returns None otherwise.
    pub fn to_yaml_value(&self) -> Option<serde_yaml::Value> {
        match self {
            Self::Yaml(yaml) => Some(yaml.clone()),
            _ => None,
        }
    }
}

impl Display for TrivialValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TrivialValue::String(s) => write!(f, "{}", s),
            TrivialValue::File(file) => write!(f, "{}", file.path),
            TrivialValue::Yaml(yaml) => write!(
                f,
                "{}",
                serde_yaml::to_string(yaml)
                    .expect("A value of type serde_yaml::Value should always be serializable")
            ),
            TrivialValue::Bool(b) => write!(f, "{}", b),
            TrivialValue::Number(n) => write!(f, "{}", n),
            TrivialValue::MapStringString(n) => {
                let flatten: Vec<String> = n
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect();
                write!(f, "{}", flatten.join(" "))
            }
            TrivialValue::MapStringFile(n) => {
                let flatten: Vec<String> = n
                    .iter()
                    .map(|(key, value)| format!("{key}={}", value.path))
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
pub enum Number {
    PosInt(u64),
    /// Always less than zero.
    NegInt(i64),
    /// May be infinite or NaN.
    Float(f64),
}

impl Display for Number {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Number::PosInt(n) => write!(f, "{}", n),
            Number::NegInt(n) => write!(f, "{}", n),
            Number::Float(n) => write!(f, "{}", n),
        }
    }
}

// /// Represents a yaml value, holding both the string before deserializing and the [serde_yaml::Value] after.
// #[derive(Debug, PartialEq, Default, Clone, Deserialize)]
// pub struct YamlValue {
//     #[serde(skip)]
//     pub value: serde_yaml::Value,
//     #[serde(flatten)]
//     pub content: String,
// }

// impl TryFrom<String> for YamlValue {
//     type Error = serde_yaml::Error;

//     fn try_from(value: String) -> Result<Self, Self::Error> {
//         Ok(Self {
//             value: serde_yaml::from_str(value.as_str())?,
//             content: value,
//         })
//     }
// }
