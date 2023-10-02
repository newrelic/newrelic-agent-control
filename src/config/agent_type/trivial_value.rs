use std::{
    collections::HashMap as Map,
    fmt::{Display, Formatter},
};

use serde::Deserialize;

use super::{agent_types::VariableType, error::AgentTypeError};

/// Represents all the allowed types for a configuration defined in the spec value.
#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(untagged)]
pub enum TrivialValue {
    String(String),
    #[serde(skip)]
    File(FilePathWithContent),
    Bool(bool),
    Number(N),
    Map(Map<String, TrivialValue>),
}

impl TrivialValue {
    /// Checks the `TrivialValue` against the given [`VariableType`], erroring if they do not match.
    ///
    /// This is also in charge of converting a `TrivialValue::String` into a `TrivialValue::File`, using the actual string as the file content, if the given [`VariableType`] is `VariableType::File`.
    pub fn check_type(self, type_: VariableType) -> Result<Self, AgentTypeError> {
        match (self.clone(), type_) {
            (TrivialValue::String(_), VariableType::String)
            | (TrivialValue::Bool(_), VariableType::Bool)
            | (TrivialValue::File(_), VariableType::File)
            | (TrivialValue::Number(_), VariableType::Number) => Ok(self),
            (TrivialValue::Map(m), VariableType::MapStringString) => {
                if !m.iter().all(|(_, v)| matches!(v, TrivialValue::String(_))) {
                    return Err(AgentTypeError::InvalidMap);
                }
                Ok(self)
            }
            (TrivialValue::String(s), VariableType::File) => {
                Ok(TrivialValue::File(FilePathWithContent::new(s)))
            }
            (v, t) => Err(AgentTypeError::TypeMismatch {
                expected_type: t,
                actual_value: v,
            }),
        }
    }
}

impl Display for TrivialValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TrivialValue::String(s) => write!(f, "{}", s),
            TrivialValue::File(file) => write!(f, "{}", file.path),
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
    pub fn new(content: String) -> Self {
        FilePathWithContent {
            content,
            ..Default::default()
        }
    }
}

/// Represents a numeric value, which can be either a positive integer, a negative integer or a float.
#[derive(Debug, PartialEq, Clone, Deserialize)]
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
