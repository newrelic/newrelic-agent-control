//! Implementation of the `agent-type validate` command.
use std::{fs, path::PathBuf};

use crate::agent_type::validation;
use crate::cli::common::error::CliError;
use tracing::info;

/// Validates an agent type definition file.
#[derive(Debug, clap::Parser)]
pub struct Args {
    /// Path to the agent type definition file to validate.
    #[arg(long, required = true)]
    file: PathBuf,
}

/// Reads the agent type file and validates its schema (required fields, field types and format
/// constraints) and semantics (every `${nr-var:X}` reference in `deployment` matches a
/// `variables` declaration, and every `${nr-xxx:...}` reference uses a known namespace),
/// reporting any issue found.
pub fn validate(args: Args) -> Result<(), CliError> {
    let content = fs::read(&args.file).map_err(|err| {
        CliError::FileSystemError(format!("reading '{}': {err}", args.file.display()))
    })?;

    validation::validate(&content).map_err(|err| {
        CliError::Validation(format!(
            "invalid agent type definition on '{}': {}",
            args.file.display(),
            err
        ))
    })?;

    info!(
        file = %args.file.display(),
        "Agent type definition is valid"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use std::io::Write;
    use tempfile::NamedTempFile;

    const VALID_HOST_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
protocol_version: "1.0"
platform: host
operating_system: linux
variables: {}
deployment: {}
"#;

    const MALFORMED_YAML: &str = "name: [unclosed";

    fn write_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("failed to write temp file");
        file
    }

    #[test]
    fn test_validate_accepts_a_valid_definition_file() {
        let file = write_file(VALID_HOST_YAML);
        let args = Args {
            file: file.path().to_path_buf(),
        };

        assert_matches!(validate(args), Ok(()));
    }

    #[test]
    fn test_validate_reports_an_invalid_definition_file() {
        let file = write_file(MALFORMED_YAML);
        let args = Args {
            file: file.path().to_path_buf(),
        };

        assert_matches!(validate(args), Err(CliError::Validation(_)));
    }

    #[test]
    fn test_validate_reports_filesystem_error_for_missing_file() {
        let args = Args {
            file: PathBuf::from("/nonexistent/path/agent-type.yaml"),
        };

        assert_matches!(validate(args), Err(CliError::FileSystemError(_)));
    }
}
