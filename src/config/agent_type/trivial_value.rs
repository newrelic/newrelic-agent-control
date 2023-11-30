use std::{
    collections::HashMap as Map,
    fmt::{Display, Formatter},
};

use serde::Deserialize;

use super::{
    agent_types::{EndSpec, VariableType},
    error::AgentTypeError,
};

/// Represents all the allowed types for a configuration defined in the spec value.
#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(untagged)]
pub enum TrivialValue {
    String(String),
    Bool(bool),
    Number(Number),
    MapStringString(Map<String, String>),

    #[serde(skip)] // Can't distinguish File from String when deserializing // FIXME
    File(FilePathWithContent),
    #[serde(skip)] // Can't distinguish StringFile from StringString when deserializing // FIXME
    MapStringFile(Map<String, FilePathWithContent>),
}

impl TrivialValue {
    /// Checks the `TrivialValue` against the given [`VariableType`], erroring if they do not match.
    ///
    /// This is also in charge of converting a `TrivialValue::String` into a `TrivialValue::File`, using the actual string as the file content, if the given [`VariableType`] is `VariableType::File`.
    pub fn check_type(&self, end_spec: &EndSpec) -> Result<(), AgentTypeError> {
        let trivial_value = self;
        let var_type = end_spec.variable_type();
        let trivial_value_str = format!("{trivial_value:?}");
        let var_type_str = format!("{var_type:?}");
        match (trivial_value, var_type) {
            (TrivialValue::String(_), VariableType::String)
            | (TrivialValue::Bool(_), VariableType::Bool)
            | (TrivialValue::String(_), VariableType::File)
            | (TrivialValue::Number(_), VariableType::Number)
            | (TrivialValue::MapStringString(_), VariableType::MapStringString)
            | (TrivialValue::MapStringString(_), VariableType::MapStringFile) => Ok(()),
            // There is a possibility that the expected type is a file or a string but the parsed value results in a MapStringString or MapStringFile.
            // FIXME is it possible to tell serde the difference? Not sure.
            // So we need to
            (v, t) => Err(AgentTypeError::TypeMismatch {
                expected_type: t.to_string(),
                actual_value: v.clone(),
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
            TrivialValue::MapStringFile(m) => {
                let flatten = m
                    .iter()
                    .fold(String::new(), |acc, (k, v)| format!("{acc} {k}={}", v.path));
                write!(f, "{}", flatten)
            }
            TrivialValue::MapStringString(m) => {
                let flatten = m
                    .iter()
                    .fold(String::new(), |acc, (k, v)| format!("{acc} {k}={v}"));
                write!(f, "{}", flatten)
            }
        }
    }
}

/// Represents a file path and its content.
#[derive(Debug, PartialEq, Default, Clone, Deserialize)]
#[serde(from = "String")]
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

impl From<String> for FilePathWithContent {
    fn from(value: String) -> Self {
        FilePathWithContent {
            path: String::default(),
            content: value,
        }
    }
}

/// Represents a numeric value, which can be either a positive integer, a negative integer or a float.
#[derive(Debug, PartialEq, Clone, Deserialize)]
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
