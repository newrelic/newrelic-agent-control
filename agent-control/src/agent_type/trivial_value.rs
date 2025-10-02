use std::{
    collections::HashMap as Map,
    fmt::{Display, Formatter},
};

use serde::{Deserialize, Serialize};

/// Represents all the allowed types for a configuration defined in the spec value.
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TrivialValue {
    String(String),
    Bool(bool),
    Number(serde_yaml::Number),
    #[serde(skip)]
    Yaml(serde_yaml::Value),
    #[serde(skip)]
    MapStringString(Map<String, String>),
    #[serde(skip)]
    MapStringYaml(Map<String, serde_yaml::Value>),
}

impl TrivialValue {
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
            TrivialValue::String(s) => write!(f, "{s}"),
            TrivialValue::Yaml(yaml) => write!(
                f,
                "{}",
                serde_yaml::to_string(yaml)
                    .expect("A value of type serde_yaml::Value should always be serializable")
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
                serde_yaml::to_string(n)
                    .expect("A value of type HashMap<String, serde_yaml::Value> should always be serializable")
            )
        }
    }
}
