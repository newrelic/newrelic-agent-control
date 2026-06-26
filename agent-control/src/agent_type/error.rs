//! Error type for operations involving agent types.
use thiserror::Error;

/// The different error types to be returned by operations involving the [`AgentType`](super::definition::AgentType) type.
#[derive(Error, Debug)]
pub enum AgentTypeError {
    /// Serializing a value to YAML failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_saphyr::Error),
    /// Converting a JSON value to the expected type failed.
    #[error("value conversion error: {0}")]
    ValueConversion(#[from] serde_json::Error),
    /// A required value is missing for the given key.
    #[error("missing value for key: {0}")]
    MissingValue(String),
    /// The value provided for a key is not of the expected shape.
    #[error("unexpected value for key: key({0}) val({1})")]
    UnexpectedValueForKey(String, String),
    /// A template references a key that is not defined.
    #[error("missing required template key: {0}")]
    MissingTemplateKey(String),
    /// Parsing the agent type variables failed.
    #[error("parsing AgentType variables: {0}")]
    Parse(String),
    /// Some required variables were not populated with a value.
    #[error("not all values for this agent type have been populated: {0:?}")]
    ValuesNotPopulated(Vec<String>),
    /// A templated string could not be parsed into the target value type.
    #[error("template value not parseable from the string {0}")]
    ValueNotParseableFromString(String),
    /// The configured backoff strategy type is not recognized.
    #[error("unknown backoff strategy type: {0}")]
    UnknownBackoffStrategyType(String),
    /// The provided value is not one of the allowed variants.
    #[error("invalid value provided. Variants allowed: {0}")]
    InvalidVariant(String),
    /// Rendering a template failed.
    #[error("rendering template: {0}")]
    RenderingTemplate(String),
    /// An OCI reference could not be parsed.
    #[error("error parsing oci reference: {0}")]
    OCIReferenceParsingError(String),
}
