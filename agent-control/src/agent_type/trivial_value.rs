//! A single configuration value as resolved from an agent type variable's spec.
use std::{
    collections::HashMap as Map,
    fmt::{Display, Formatter},
};

use serde::{Deserialize, Serialize};

/// Represents all the allowed types for a configuration defined in the spec value.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TrivialValue {
    /// A string value.
    String(String),
    /// A boolean value.
    Bool(bool),
    /// A numeric value.
    Number(serde_json::Number),
    /// An arbitrary YAML value.
    #[serde(skip)]
    Yaml(serde_json::Value),
    /// A map of string keys to string values.
    #[serde(skip)]
    MapStringString(Map<String, String>),
    /// A map of string keys to arbitrary YAML values.
    #[serde(skip)]
    MapStringYaml(Map<String, serde_json::Value>),
}

impl TrivialValue {
    /// If the trivial value is a yaml, it returns a copy the corresponding [serde_json::Value], returns None otherwise.
    pub fn to_yaml_value(&self) -> Option<serde_json::Value> {
        match self {
            Self::Yaml(yaml) => Some(yaml.clone()),
            _ => None,
        }
    }
}

impl Display for TrivialValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TrivialValue::String(s) => write!(f, "{s}"),
            TrivialValue::Yaml(yaml) => write!(
                f,
                "{}",
                serde_saphyr::to_string(yaml)
                    .expect("A value of type serde_json::Value should always be serializable")
            ),
            TrivialValue::Bool(b) => write!(f, "{b}"),
            TrivialValue::Number(n) => write!(f, "{n}"),
            TrivialValue::MapStringString(n) => {
                let flatten: Vec<String> = n
                    .iter()
                    // FIXME is this what we really want? key=value?
                    .map(|(key, value)| format!("{key}={value}")) 
                    .collect();
                write!(f, "{}", flatten.join(" "))
            }
            TrivialValue::MapStringYaml(n) => write!(
                f,
                "{}",
                serde_saphyr::to_string(n)
                    .expect("A value of type HashMap<String, serde_json::Value> should always be serializable")
            )
        }
    }
}
