use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent_type_definition::{
    error::AgentTypeError,
    trivial_value::{FilePathWithContent, TrivialValue},
};

use super::kind_value::{KindValue, KindValueWithPath};

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum Kind {
    #[serde(rename = "string")]
    String(KindValue<String>),
    #[serde(rename = "bool")]
    Bool(KindValue<bool>),
    #[serde(rename = "number")]
    Number(KindValue<serde_yaml::Number>),
    #[serde(rename = "file")]
    File(KindValueWithPath<FilePathWithContent>),
    #[serde(rename = "map[string]string")]
    MapStringString(KindValue<HashMap<String, String>>),
    #[serde(rename = "map[string]file")]
    MapStringFile(KindValueWithPath<HashMap<String, FilePathWithContent>>),
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

impl From<KindValue<serde_yaml::Number>> for Kind {
    fn from(kind_value: KindValue<serde_yaml::Number>) -> Self {
        Kind::Number(kind_value)
    }
}

impl From<KindValueWithPath<FilePathWithContent>> for Kind {
    fn from(kind_value: KindValueWithPath<FilePathWithContent>) -> Self {
        Kind::File(kind_value)
    }
}

impl From<KindValue<HashMap<String, String>>> for Kind {
    fn from(kind_value: KindValue<HashMap<String, String>>) -> Self {
        Kind::MapStringString(kind_value)
    }
}

impl From<KindValueWithPath<HashMap<String, FilePathWithContent>>> for Kind {
    fn from(kind_value: KindValueWithPath<HashMap<String, FilePathWithContent>>) -> Self {
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
    pub(crate) fn is_required(&self) -> bool {
        match self {
            Kind::String(k) => k.required,
            Kind::Bool(k) => k.required,
            Kind::Number(k) => k.required,
            Kind::File(k) => k.inner.required,
            Kind::MapStringString(k) => k.required,
            Kind::MapStringFile(k) => k.inner.required,
            Kind::Yaml(k) => k.required,
        }
    }

    pub(crate) fn merge_with_yaml_value(
        &mut self,
        value: serde_yaml::Value,
    ) -> Result<(), AgentTypeError> {
        match self {
            Kind::String(kv) => kv.set_final_value(serde_yaml::from_value(value)?),
            Kind::Bool(kv) => kv.set_final_value(serde_yaml::from_value(value)?),
            Kind::Number(kv) => kv.set_final_value(serde_yaml::from_value(value)?),
            Kind::File(kv) => {
                let mut file: FilePathWithContent = serde_yaml::from_value(value)?;
                file.with_path(kv.file_path.clone());
                kv.inner.set_final_value(file)
            }
            Kind::MapStringString(kv) => kv.set_final_value(serde_yaml::from_value(value)?),
            Kind::MapStringFile(kv) => {
                let mut files: HashMap<String, FilePathWithContent> =
                    serde_yaml::from_value(value)?;
                files
                    .values_mut()
                    .for_each(|f| f.with_path(kv.file_path.clone()));
                kv.inner.set_final_value(files)
            }
            Kind::Yaml(kv) => kv.set_final_value(value),
        }?;
        Ok(())
    }

    pub(crate) fn get_final_value(&self) -> Option<TrivialValue> {
        match self {
            Kind::String(k) => k
                .final_value
                .as_ref()
                .or(k.default.as_ref())
                .cloned()
                .map(TrivialValue::String),
            Kind::Bool(k) => k.final_value.or(k.default).map(TrivialValue::Bool),
            Kind::Number(k) => k
                .final_value
                .as_ref()
                .or(k.default.as_ref())
                .cloned()
                .map(TrivialValue::Number),
            Kind::File(k) => k
                .inner
                .final_value
                .as_ref()
                .or({
                    let mut file = k.inner.default.clone();
                    if let Some(f) = file.as_mut() {
                        f.with_path(k.file_path.clone())
                    }
                    file
                }
                .as_ref())
                .cloned()
                .map(TrivialValue::File),
            Kind::MapStringString(k) => k
                .final_value
                .as_ref()
                .or(k.default.as_ref())
                .cloned()
                .map(TrivialValue::MapStringString),
            Kind::MapStringFile(k) => k
                .inner
                .final_value
                .as_ref()
                .or(k.inner.default.as_ref())
                .cloned()
                .map(TrivialValue::MapStringFile),
            Kind::Yaml(k) => k
                .final_value
                .as_ref()
                .or(k.default.as_ref())
                .cloned()
                .map(TrivialValue::Yaml),
        }
    }

    pub(crate) fn get_file_path(&self) -> Option<&PathBuf> {
        match self {
            Kind::File(k) => Some(k.get_file_path()),
            Kind::MapStringFile(k) => Some(k.get_file_path()),
            _ => None,
        }
    }

    pub(crate) fn set_file_path(&mut self, file_path: PathBuf) {
        match self {
            Kind::File(k) => k.set_file_path(file_path),
            Kind::MapStringFile(k) => k.set_file_path(file_path),
            _ => {}
        }
    }
}
