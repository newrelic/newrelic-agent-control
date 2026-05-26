use thiserror::Error;

/// The different error types to be returned by operations involving the [`Agent`] type.
#[derive(Error, Debug)]
pub enum AgentTypeError {
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_saphyr::Error),
    #[error("value conversion error: {0}")]
    ValueConversion(#[from] serde_json::Error),
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
    #[error("error parsing oci reference: {0}")]
    OCIReferenceParsingError(String),
}
