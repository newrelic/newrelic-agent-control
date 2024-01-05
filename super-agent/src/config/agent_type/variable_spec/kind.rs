use std::collections::HashMap;

use serde::Deserialize;

use crate::config::agent_type::trivial_value::{FilePathWithContent, Number, TrivialValue};

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

/// The below methods are mostly concerned with delegating to the same method of the inner type
/// on each `Kind` variant. It's a lot of boilerplate, but declarative and easy to get.
impl Kind {
    pub(crate) fn is_required(&self) -> bool {
        match self {
            Kind::String(kind_value) => kind_value.required,
            Kind::Bool(kind_value) => kind_value.required,
            Kind::Number(kind_value) => kind_value.required,
            Kind::File(kind_value) => kind_value.required,
            Kind::MapStringString(kind_value) => kind_value.required,
            Kind::MapStringFile(kind_value) => kind_value.required,
            Kind::Yaml(kind_value) => kind_value.required,
        }
    }

    pub(crate) fn is_not_required_without_default(&self) -> bool {
        match self {
            Kind::String(kind_value) => kind_value.not_required_without_default(),
            Kind::Bool(kind_value) => kind_value.not_required_without_default(),
            Kind::Number(kind_value) => kind_value.not_required_without_default(),
            Kind::File(kind_value) => kind_value.not_required_without_default(),
            Kind::MapStringString(kind_value) => kind_value.not_required_without_default(),
            Kind::MapStringFile(kind_value) => kind_value.not_required_without_default(),
            Kind::Yaml(kind_value) => kind_value.not_required_without_default(),
        }
    }

    pub(crate) fn set_default_as_final(&mut self) {
        match self {
            Kind::String(kind_value) => kind_value.set_default_as_final(),
            Kind::Bool(kind_value) => kind_value.set_default_as_final(),
            Kind::Number(kind_value) => kind_value.set_default_as_final(),
            Kind::File(kind_value) => kind_value.set_default_as_final(),
            Kind::MapStringString(kind_value) => kind_value.set_default_as_final(),
            Kind::MapStringFile(kind_value) => kind_value.set_default_as_final(),
            Kind::Yaml(kind_value) => kind_value.set_default_as_final(),
        }
    }

    pub(crate) fn set_final_value(&mut self, final_value: TrivialValue) {
        match (self, final_value) {
            (Kind::String(k), TrivialValue::String(v)) => {
                k.final_value = Some(v)
            }
            (Kind::Bool(k), TrivialValue::Bool(v)) => {
                k.final_value = Some(v)
            }
            (Kind::Number(k), TrivialValue::Number(v)) => {
                k.final_value = Some(v)
            }
            (Kind::File(k), TrivialValue::File(v)) => {
                k.final_value = Some(v)
            }
            (Kind::MapStringString(k), TrivialValue::MapStringString(v)) => {
                k.final_value = Some(v)
            }
            (Kind::MapStringFile(k), TrivialValue::MapStringFile(v)) => {
                k.final_value = Some(v)
            }
            (Kind::Yaml(k), TrivialValue::Yaml(v)) => {
                k.final_value = Some(v)
            }
            _ => panic!("Invalid final value"),
        }
        // match self {
        //     Kind::String(kind_value) => kind_value.final_value = Some(final_value),
        //     Kind::Bool(kind_value) => kind_value.final_value = Some(final_value),
        //     Kind::Number(kind_value) => kind_value.final_value = Some(final_value),
        //     Kind::File(kind_value) => kind_value.final_value = Some(final_value),
        //     Kind::MapStringString(kind_value) => kind_value.final_value = Some(final_value),
        //     Kind::MapStringFile(kind_value) => kind_value.final_value = Some(final_value),
        //     Kind::Yaml(kind_value) => kind_value.final_value = Some(final_value),
        // }
    }
}
