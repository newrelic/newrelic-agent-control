//! Implementation of the `agent-type validate` command.
use std::{fs, path::PathBuf};

use crate::agent_type::definition::AgentTypeDefinition;
use crate::agent_type::semantic_validation;
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
/// `variables` declaration), reporting any issue found.
pub fn validate(args: Args) -> Result<(), CliError> {
    let content = fs::read(&args.file).map_err(|err| {
        CliError::FileSystemError(format!("reading '{}': {err}", args.file.display()))
    })?;

    let definition = AgentTypeDefinition::from_slice(&content).map_err(|err| {
        CliError::Validation(format!(
            "invalid agent type definition on '{}': {}",
            args.file.display(),
            err
        ))
    })?;

    semantic_validation::validate_variable_references(&content, &definition.variables).map_err(
        |err| {
            CliError::Validation(format!(
                "invalid agent type definition on '{}': {}",
                args.file.display(),
                err
            ))
        },
    )?;

    info!(
        agent_type = %definition.agent_type_id(),
        file = %args.file.display(),
        "Agent type definition is valid"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use rstest::rstest;
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

    const VALID_K8S_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
protocol_version: "1.0"
platform: kubernetes
variables: {}
deployment:
  objects: {}
"#;

    const MISSING_REQUIRED_FIELD_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
protocol_version: "1.0"
platform: kubernetes
"#;

    const MALFORMED_YAML: &str = "name: [unclosed";

    const MISSING_PROTOCOL_VERSION_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
platform: kubernetes
variables: {}
deployment:
  objects: {}
"#;

    // A far-future version is newer than this Agent Control understands.
    const TOO_NEW_PROTOCOL_VERSION_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
protocol_version: "9999.0"
platform: kubernetes
variables: {}
deployment:
  objects: {}
"#;

    const SEMANTIC_UNDECLARED_REFERENCE_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
protocol_version: "1.0"
platform: kubernetes
variables: {}
deployment:
  objects:
    cr1:
      apiVersion: fake.group/v1beta1
      kind: FakeKind
      metadata:
        name: fake-object
        namespace: fake-object-namespace
      spec:
        image: ${nr-var:image}
"#;

    const VALID_K8S_WITH_UNUSED_VARIABLE_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
protocol_version: "1.0"
platform: kubernetes
variables:
  image:
    description: "container image"
    type: string
    required: true
deployment:
  objects: {}
"#;

    fn write_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("failed to write temp file");
        file
    }

    #[rstest]
    #[case::host(VALID_HOST_YAML)]
    #[case::k8s(VALID_K8S_YAML)]
    #[case::k8s_with_unused_variable(VALID_K8S_WITH_UNUSED_VARIABLE_YAML)]
    fn test_validate_accepts_valid_definitions(#[case] yaml: &str) {
        let file = write_file(yaml);
        let args = Args {
            file: file.path().to_path_buf(),
        };

        assert_matches!(validate(args), Ok(()));
    }

    #[rstest]
    #[case::missing_field(MISSING_REQUIRED_FIELD_YAML)]
    #[case::malformed_yaml(MALFORMED_YAML)]
    #[case::missing_protocol_version(MISSING_PROTOCOL_VERSION_YAML)]
    #[case::too_new_protocol_version(TOO_NEW_PROTOCOL_VERSION_YAML)]
    #[case::undeclared_variable_reference(SEMANTIC_UNDECLARED_REFERENCE_YAML)]
    fn test_validate_rejects_invalid_definitions(#[case] yaml: &str) {
        let file = write_file(yaml);
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
