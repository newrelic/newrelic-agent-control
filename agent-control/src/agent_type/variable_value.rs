//! A single configuration value as resolved from an agent type variable's spec.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap as Map,
    fmt::{Display, Formatter},
};

/// The supported values for the `type` field of an Agent Type variable.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum VariableType {
    /// A string-typed variable.
    #[serde(rename = "string")]
    String,
    /// A boolean-typed variable.
    #[serde(rename = "bool")]
    Bool,
    /// A number-typed variable.
    #[serde(rename = "number")]
    Number,
    /// A map of string keys to string values.
    /// A merged value that isn't already a string is accepted and encoded as its YAML text form.
    #[serde(rename = "string_map")]
    StringMap,
    /// A yaml-typed variable.
    ///
    /// When the variable is optional and the user omits `default`, VariableDefinition's deserializer
    /// fills the default with the YAML `null` value — the natural absence representation for a yaml-typed variable.
    #[serde(rename = "yaml")]
    Yaml,
}

/// Represents all the allowed types for a configuration defined in the spec value.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum VariableValue {
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
}

impl Display for VariableValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            VariableValue::String(s) => write!(f, "{s}"),
            VariableValue::Yaml(yaml) => write!(
                f,
                "{}",
                serde_saphyr::to_string(yaml)
                    .expect("A value of type serde_json::Value should always be serializable")
            ),
            VariableValue::Bool(b) => write!(f, "{b}"),
            VariableValue::Number(n) => write!(f, "{n}"),
            // Serialized as YAML text: `dir_content_from_map` re-parses this as a YAML mapping.
            VariableValue::MapStringString(n) => write!(
                f,
                "{}",
                serde_saphyr::to_string(n).expect(
                    "A value of type HashMap<String, String> should always be serializable"
                )
            ),
        }
    }
}
