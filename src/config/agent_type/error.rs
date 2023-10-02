use std::io;

use thiserror::Error;

use super::{agent_types::VariableType, trivial_value::TrivialValue};

/// The different error types to be returned by operations involving the [`Agent`] type.
#[derive(Error, Debug)]
pub enum AgentTypeError {
    #[error("`{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error("Missing required key in config: `{0}`")]
    MissingAgentKey(String),
    #[error(
        "Type mismatch while parsing. Expected type {expected_type:?}, got value {actual_value:?}"
    )]
    TypeMismatch {
        expected_type: VariableType,
        actual_value: TrivialValue,
    },
    #[error("Found unexpected keys in config: {0:?}")]
    UnexpectedKeysInConfig(Vec<String>),
    #[error("I/O error: `{0}`")]
    IOError(#[from] io::Error),
    #[error("Attempted to store an invalid path on a FilePathWithContent object")]
    InvalidFilePath,
    #[error("Missing required template key: `{0}`")]
    MissingTemplateKey(String),

    #[error("Map values must be of the same type")]
    InvalidMap,

    #[error("Missing default value for a non-required spec key")]
    MissingDefault,
    #[error("Missing default value for spec key `{0}`")]
    MissingDefaultWithKey(String),
    #[error("Invalid default value for spec key `{key}`: expected a {type_:?}")]
    InvalidDefaultForSpec { key: String, type_: VariableType },

    #[error("Value was not populated")]
    ValueNotPopulated,

    #[error("Template value not parseable from a String")]
    ValueNotParseableFromString,
}
