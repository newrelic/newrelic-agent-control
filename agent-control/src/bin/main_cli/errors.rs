use std::process::ExitCode;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("Failed to apply resource: {0}")]
    ApplyResource(String),

    #[error("Failed to parse data: {0}")]
    Parse(#[from] ParseError),
}

impl CliError {
    pub fn to_exit_code(&self) -> ExitCode {
        match self {
            CliError::ApplyResource(_) => ExitCode::from(1),
            CliError::Parse(err) => err.to_exit_code(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Failed to parse yaml: {0}")]
    YamlString(String),

    #[error("Failed to parse file: {0}")]
    FileParse(String),
}

impl ParseError {
    pub fn to_exit_code(&self) -> ExitCode {
        match self {
            ParseError::YamlString(_) => ExitCode::from(65),
            ParseError::FileParse(_) => ExitCode::from(66),
        }
    }
}
