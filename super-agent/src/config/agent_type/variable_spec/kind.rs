use std::collections::HashMap;

use serde::Deserialize;

use crate::config::agent_type::{
    error::AgentTypeError,
    trivial_value::{FilePathWithContent, Number, TrivialValue},
};

use super::kind_value::KindValue;

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Kind {
    #[serde(rename = "string")]
    String(KindValue<String>),
    #[serde(rename = "bool")]
    Bool(KindValue<bool>),
    #[serde(rename = "number")]
    Number(KindValue<Number>),
    #[serde(rename = "file")]
    File(KindValue<FilePathWithContent>),
    #[serde(rename = "map[string]string")]
    MapStringString(KindValue<HashMap<String, String>>),
    #[serde(rename = "map[string]file")]
    MapStringFile(KindValue<HashMap<String, FilePathWithContent>>),
    #[serde(rename = "yaml")]
    Yaml(KindValue<serde_yaml::Value>),
}

/// Conversions from KindValue<T> to Kind

impl From<KindValue<String>> for Kind {
    fn from(kind_value: KindValue<String>) -> Self {
        Kind::String(kind_value)
    }
}

impl From<KindValue<bool>> for Kind {
    fn from(kind_value: KindValue<bool>) -> Self {
        Kind::Bool(kind_value)
    }
}

impl From<KindValue<Number>> for Kind {
    fn from(kind_value: KindValue<Number>) -> Self {
        Kind::Number(kind_value)
    }
}

impl From<KindValue<FilePathWithContent>> for Kind {
    fn from(kind_value: KindValue<FilePathWithContent>) -> Self {
        Kind::File(kind_value)
    }
}

impl From<KindValue<HashMap<String, String>>> for Kind {
    fn from(kind_value: KindValue<HashMap<String, String>>) -> Self {
        Kind::MapStringString(kind_value)
    }
}

impl From<KindValue<HashMap<String, FilePathWithContent>>> for Kind {
    fn from(kind_value: KindValue<HashMap<String, FilePathWithContent>>) -> Self {
        Kind::MapStringFile(kind_value)
    }
}

impl From<KindValue<serde_yaml::Value>> for Kind {
    fn from(kind_value: KindValue<serde_yaml::Value>) -> Self {
        Kind::Yaml(kind_value)
    }
}

/// The below methods are mostly concerned with delegating to the inner type on each `Kind` variant.
/// It's a lot of boilerplate, but declarative and straight-forward.
impl Kind {
    pub(crate) fn name(&self) -> &str {
        match self {
            Kind::String(_) => "string",
            Kind::Bool(_) => "bool",
            Kind::Number(_) => "number",
            Kind::File(_) => "file",
            Kind::MapStringString(_) => "map[string]string",
            Kind::MapStringFile(_) => "map[string]file",
            Kind::Yaml(_) => "yaml",
        }
    }
    pub(crate) fn is_required(&self) -> bool {
        match self {
            Kind::String(k) => k.required,
            Kind::Bool(k) => k.required,
            Kind::Number(k) => k.required,
            Kind::File(k) => k.required,
            Kind::MapStringString(k) => k.required,
            Kind::MapStringFile(k) => k.required,
            Kind::Yaml(k) => k.required,
        }
    }

    pub(crate) fn is_not_required_without_default(&self) -> bool {
        match self {
            Kind::String(k) => k.not_required_without_default(),
            Kind::Bool(k) => k.not_required_without_default(),
            Kind::Number(k) => k.not_required_without_default(),
            Kind::File(k) => k.not_required_without_default(),
            Kind::MapStringString(k) => k.not_required_without_default(),
            Kind::MapStringFile(k) => k.not_required_without_default(),
            Kind::Yaml(k) => k.not_required_without_default(),
        }
    }

    pub(crate) fn set_default_as_final(&mut self) {
        match self {
            Kind::String(k) => k.set_default_as_final(),
            Kind::Bool(k) => k.set_default_as_final(),
            Kind::Number(k) => k.set_default_as_final(),
            Kind::File(k) => k.set_default_as_final(),
            Kind::MapStringString(k) => k.set_default_as_final(),
            Kind::MapStringFile(k) => k.set_default_as_final(),
            Kind::Yaml(k) => k.set_default_as_final(),
        }
    }

    pub(crate) fn set_final_value(
        &mut self,
        final_value: TrivialValue,
    ) -> Result<(), AgentTypeError> {
        match (self, final_value) {
            (Kind::String(k), TrivialValue::String(v)) => k.final_value = Some(v),
            (Kind::Bool(k), TrivialValue::Bool(v)) => k.final_value = Some(v),
            (Kind::Number(k), TrivialValue::Number(v)) => k.final_value = Some(v),
            (Kind::File(k), TrivialValue::File(v)) => k.final_value = Some(v),
            (Kind::MapStringString(k), TrivialValue::MapStringString(v)) => k.final_value = Some(v),
            (Kind::MapStringFile(k), TrivialValue::MapStringFile(v)) => k.final_value = Some(v),
            (Kind::Yaml(k), TrivialValue::Yaml(v)) => k.final_value = Some(v),
            (k, v) => {
                return Err(AgentTypeError::TypeMismatch {
                    expected_type: k.name(),
                    actual_value: v,
                })
            }
        }
        Ok(())
    }
}
