use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::agent_type::{
    agent_types::VariableType,
    error::AgentTypeError,
    trivial_value::{FilePathWithContent, Number, TrivialValue},
};

use super::kind_value::{KindValue, KindValueWithPath};

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub(super) enum Kind {
    #[serde(rename = "string")]
    String(KindValue<String>),
    #[serde(rename = "bool")]
    Bool(KindValue<bool>),
    #[serde(rename = "number")]
    Number(KindValue<Number>),
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

impl From<KindValue<Number>> for Kind {
    fn from(kind_value: KindValue<Number>) -> Self {
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

/// Conversions from Kind to KindValue<T>

impl TryFrom<&Kind> for KindValue<String> {
    type Error = AgentTypeError;

    fn try_from(kind: &Kind) -> Result<Self, Self::Error> {
        match kind {
            Kind::String(k) => Ok(k.clone()),
            _ => Err(AgentTypeError::TypeMismatch {
                expected_type: VariableType::String,
                actual_value: kind.clone().get_final_value().unwrap(),
            }),
        }
    }
}

impl TryFrom<&Kind> for KindValue<bool> {
    type Error = AgentTypeError;

    fn try_from(kind: &Kind) -> Result<Self, Self::Error> {
        match kind {
            Kind::Bool(k) => Ok(k.clone()),
            _ => Err(AgentTypeError::TypeMismatch {
                expected_type: VariableType::Bool,
                actual_value: kind.clone().get_final_value().unwrap(),
            }),
        }
    }
}

impl TryFrom<&Kind> for KindValue<Number> {
    type Error = AgentTypeError;

    fn try_from(kind: &Kind) -> Result<Self, Self::Error> {
        match kind {
            Kind::Number(k) => Ok(k.clone()),
            _ => Err(AgentTypeError::TypeMismatch {
                expected_type: VariableType::Number,
                actual_value: kind.clone().get_final_value().unwrap(),
            }),
        }
    }
}

impl TryFrom<&Kind> for KindValueWithPath<FilePathWithContent> {
    type Error = AgentTypeError;

    fn try_from(kind: &Kind) -> Result<Self, Self::Error> {
        match kind {
            Kind::File(k) => Ok(k.clone()),
            _ => Err(AgentTypeError::TypeMismatch {
                expected_type: VariableType::File,
                actual_value: kind.clone().get_final_value().unwrap(),
            }),
        }
    }
}

impl TryFrom<&Kind> for KindValue<HashMap<String, String>> {
    type Error = AgentTypeError;

    fn try_from(kind: &Kind) -> Result<Self, Self::Error> {
        match kind {
            Kind::MapStringString(k) => Ok(k.clone()),
            _ => Err(AgentTypeError::TypeMismatch {
                expected_type: VariableType::MapStringString,
                actual_value: kind.clone().get_final_value().unwrap(),
            }),
        }
    }
}

impl TryFrom<&Kind> for KindValueWithPath<HashMap<String, FilePathWithContent>> {
    type Error = AgentTypeError;

    fn try_from(kind: &Kind) -> Result<Self, Self::Error> {
        match kind {
            Kind::MapStringFile(k) => Ok(k.clone()),
            _ => Err(AgentTypeError::TypeMismatch {
                expected_type: VariableType::MapStringFile,
                actual_value: kind.clone().get_final_value().unwrap(),
            }),
        }
    }
}

impl TryFrom<&Kind> for KindValue<serde_yaml::Value> {
    type Error = AgentTypeError;

    fn try_from(kind: &Kind) -> Result<Self, Self::Error> {
        match kind {
            Kind::Yaml(k) => Ok(k.clone()),
            _ => Err(AgentTypeError::TypeMismatch {
                expected_type: VariableType::Yaml,
                actual_value: kind.clone().get_final_value().unwrap(),
            }),
        }
    }
}

/// The below methods are mostly concerned with delegating to the inner type on each `Kind` variant.
/// It's a lot of boilerplate, but declarative and straight-forward.
impl Kind {
    pub(crate) fn variable_type(&self) -> VariableType {
        match self {
            Kind::String(_) => VariableType::String,
            Kind::Bool(_) => VariableType::Bool,
            Kind::Number(_) => VariableType::Number,
            Kind::File(_) => VariableType::File,
            Kind::MapStringString(_) => VariableType::MapStringString,
            Kind::MapStringFile(_) => VariableType::MapStringFile,
            Kind::Yaml(_) => VariableType::Yaml,
        }
    }
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

    pub(crate) fn is_not_required_without_default(&self) -> bool {
        match self {
            Kind::String(k) => k.not_required_without_default(),
            Kind::Bool(k) => k.not_required_without_default(),
            Kind::Number(k) => k.not_required_without_default(),
            Kind::File(k) => k.inner.not_required_without_default(),
            Kind::MapStringString(k) => k.not_required_without_default(),
            Kind::MapStringFile(k) => k.inner.not_required_without_default(),
            Kind::Yaml(k) => k.not_required_without_default(),
        }
    }

    // pub(crate) fn set_default_as_final(&mut self) {
    //     match self {
    //         Kind::String(k) => k.set_default_as_final(),
    //         Kind::Bool(k) => k.set_default_as_final(),
    //         Kind::Number(k) => k.set_default_as_final(),
    //         Kind::File(k) => k.inner.set_default_as_final(),
    //         Kind::MapStringString(k) => k.set_default_as_final(),
    //         Kind::MapStringFile(k) => k.inner.set_default_as_final(),
    //         Kind::Yaml(k) => k.set_default_as_final(),
    //     }
    // }

    // pub(crate) fn set_final_value(
    //     &mut self,
    //     final_value: TrivialValue,
    // ) -> Result<(), AgentTypeError> {
    //     match (self, final_value) {
    //         (Kind::String(k), TrivialValue::String(v)) => k.final_value = Some(v),
    //         (Kind::Bool(k), TrivialValue::Bool(v)) => k.final_value = Some(v),
    //         (Kind::Number(k), TrivialValue::Number(v)) => k.final_value = Some(v),
    //         (Kind::File(k), TrivialValue::File(v)) => k.inner.final_value = Some(v),
    //         (Kind::MapStringString(k), TrivialValue::MapStringString(v)) => k.final_value = Some(v),
    //         (Kind::MapStringFile(k), TrivialValue::MapStringFile(v)) => {
    //             k.inner.final_value = Some(v)
    //         }
    //         (Kind::Yaml(k), TrivialValue::Yaml(v)) => k.final_value = Some(v),
    //         (k, v) => {
    //             return Err(AgentTypeError::TypeMismatch {
    //                 expected_type: k.variable_type(),
    //                 actual_value: v,
    //             })
    //         }
    //     }
    //     Ok(())
    // }

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
        };
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
            // FIXME: This is bulls**t. Use KindValueWithFilePath which does not allow for empty paths
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
            Kind::File(k) => Some(&k.file_path),
            Kind::MapStringFile(k) => Some(&k.file_path),
            _ => None,
        }
    }

    pub(crate) fn set_file_path(&mut self, file_path: PathBuf) {
        match self {
            Kind::File(k) => k.file_path = file_path,
            Kind::MapStringFile(k) => k.file_path = file_path,
            _ => {}
        }
    }
}
