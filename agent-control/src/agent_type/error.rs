use crate::sub_agent::persister::config_persister::PersistError;
use std::io;
use thiserror::Error;

/// The different error types to be returned by operations involving the [`Agent`] type.
#[derive(Error, Debug)]
pub enum AgentTypeError {
    #[error("Error while parsing: `{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error("Missing value for key: `{0}`")]
    MissingValue(String),
    #[error("Unexpected key in agent type config values: {0}")]
    UnexpectedValueKey(String),
    #[error("Unexpected value for key: key({0}) val({1})")]
    UnexpectedValueForKey(String, String),
    #[error("I/O error: `{0}`")]
    IOError(#[from] io::Error),
    #[error("Missing required template key: `{0}`")]
    MissingTemplateKey(String),
    #[error("Missing default value for a non-required spec key")]
    MissingDefault,
    #[error("Not all values for this agent type have been populated: {0:?}")]
    ValuesNotPopulated(Vec<String>),
    #[error("Template value not parseable from the string `{0}")]
    ValueNotParseableFromString(String),
    #[error("Unknown backoff strategy type: `{0}`")]
    UnknownBackoffStrategyType(String),
    #[error("Invalid variant provided as a value: `{0}`. Variants allowed: {1:?}")]
    InvalidVariant(String, Vec<String>),
    #[error("error assembling agents: `{0}`")]
    ConfigurationPersisterError(#[from] PersistError),
    #[error("Conflicting variable definition: `{0}`")]
    ConflictingVariableDefinition(String),
}
