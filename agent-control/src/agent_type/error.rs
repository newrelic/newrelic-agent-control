use crate::agent_type::render::persister::config_persister::PersistError;
use thiserror::Error;

/// The different error types to be returned by operations involving the [`Agent`] type.
#[derive(Error, Debug)]
pub enum AgentTypeError {
    #[error("error while parsing: {0}")]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error("missing value for key: {0}")]
    MissingValue(String),
    #[error("unexpected value for key: key({0}) val({1})")]
    UnexpectedValueForKey(String, String),
    #[error("missing required template key: {0}")]
    MissingTemplateKey(String),
    #[error("parsing AgentType variables: {0}")]
    Parse(String),
    #[error("not all values for this agent type have been populated: {0:?}")]
    ValuesNotPopulated(Vec<String>),
    #[error("template value not parseable from the string {0}")]
    ValueNotParseableFromString(String),
    #[error("unknown backoff strategy type: {0}")]
    UnknownBackoffStrategyType(String),
    #[error("invalid value provided. Variants allowed: {0}")]
    InvalidVariant(String),
    #[error("rendering template: {0}")]
    RenderingTemplate(String),
    #[error("error assembling agents: {0}")]
    ConfigurationPersisterError(#[from] PersistError),
    #[error("conflicting variable definition: {0}")]
    ConflictingVariableDefinition(String),
}
