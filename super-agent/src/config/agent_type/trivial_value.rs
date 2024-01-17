use std::{
    collections::HashMap as Map,
    fmt::{Display, Formatter},
    path::PathBuf,
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

impl From<String> for TrivialValue {
    fn from(s: String) -> Self {
        TrivialValue::String(s)
    }
}

impl From<FilePathWithContent> for TrivialValue {
    fn from(file: FilePathWithContent) -> Self {
        TrivialValue::File(file)
    }
}

impl From<serde_yaml::Value> for TrivialValue {
    fn from(yaml: serde_yaml::Value) -> Self {
        TrivialValue::Yaml(yaml)
    }
}

impl From<bool> for TrivialValue {
    fn from(b: bool) -> Self {
        TrivialValue::Bool(b)
    }
}

impl From<Number> for TrivialValue {
    fn from(n: Number) -> Self {
        TrivialValue::Number(n)
    }
}

impl From<Map<String, String>> for TrivialValue {
    fn from(map: Map<String, String>) -> Self {
        TrivialValue::MapStringString(map)
    }
}

impl From<Map<String, FilePathWithContent>> for TrivialValue {
    fn from(map: Map<String, FilePathWithContent>) -> Self {
        TrivialValue::MapStringFile(map)
    }
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
            TrivialValue::File(file) => write!(f, "{}", file.path.to_string_lossy()),
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
                    .map(|(key, value)| format!("{key}={}", value.path.to_string_lossy()))
                    .collect();
                write!(f, "{}", flatten.join(" "))
            }
        }
    }
}

/// Represents a file path and its content.
#[derive(Debug, PartialEq, Default, Clone, Deserialize, Serialize)]
#[serde(from = "String")]
#[serde(into = "String")]
pub struct FilePathWithContent {
    #[serde(skip)]
    pub path: PathBuf,
    #[serde(flatten)]
    pub content: String,
}

impl FilePathWithContent {
    pub fn new(path: PathBuf, content: String) -> Self {
        FilePathWithContent { path, content }
    }
    pub fn with_path(&mut self, path: PathBuf) {
        self.path = path;
    }
}

// The minimum information needed to create a FilePathWithContent is the contents
impl From<String> for FilePathWithContent {
    fn from(content: String) -> Self {
        FilePathWithContent {
            content,
            ..Default::default()
        }
    }
}

impl From<FilePathWithContent> for String {
    fn from(file: FilePathWithContent) -> Self {
        file.content
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

impl From<serde_yaml::Number> for Number {
    fn from(n: serde_yaml::Number) -> Self {
        if n.is_u64() {
            Number::PosInt(n.as_u64().expect("Number must be convertible to u64"))
        } else if n.is_i64() {
            Number::NegInt(n.as_i64().expect("Number must be convertible to i64"))
        } else {
            Number::Float(n.as_f64().expect("Number must be convertible to f64"))
        }
    }
}

#[cfg(test)]
mod test {
    use super::FilePathWithContent;

    #[test]
    fn test_file_path_with_contents() {
        let file = FilePathWithContent::new("path".into(), "file_content".to_string());
        assert_eq!(serde_yaml::to_string(&file).unwrap(), "file_content\n");
    }
}
