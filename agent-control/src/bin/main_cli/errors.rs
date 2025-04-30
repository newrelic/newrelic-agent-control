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
    /// Converts the error to an exit code.
    /// 
    /// We comply with the [Advanced Bash Scripting Guide] and
    /// [BSD guidelines] for the exit codes.
    /// 
    /// [Advanced Bash Scripting Guide]: https://tldp.org/LDP/abs/html/exitcodes.html
    /// [BSD exit codes]: https://man.freebsd.org/cgi/man.cgi?query=sysexits&manpath=FreeBSD+4.3-RELEASE
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
    fn to_exit_code(&self) -> ExitCode {
        match self {
            ParseError::YamlString(_) => ExitCode::from(65),
            ParseError::FileParse(_) => ExitCode::from(66),
        }
    }
}
