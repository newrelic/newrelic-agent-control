//! Semantic validation for agent type definitions.
//!
//! Checks performed:
//! - **Undeclared variable**: every `${nr-var:X}` reference found anywhere in
//!   `deployment` must have a matching declaration in `variables`.
use std::collections::BTreeSet;

use serde::Deserialize;
use thiserror::Error;

use crate::agent_type::definition::VariableDefinitionTree;
use crate::agent_type::templates::template_re;
use crate::agent_type::variable::namespace::Namespace;

#[derive(Error, Debug)]
pub enum SemanticValidationError {
    #[error("failed to parse deployment section: {0}")]
    Deserialize(#[from] serde_saphyr::Error),
    #[error(
        "undeclared variable(s) referenced in deployment: {}",
        .0.iter().map(String::as_str).collect::<Vec<_>>().join(", ")
    )]
    UndeclaredVariables(BTreeSet<String>),
}

/// Validates that every `${nr-var:X}` reference in `deployment` has a matching declaration in
/// `variables`.
pub fn validate_variable_references(
    raw_agent_type_content: &[u8],
    variables: &VariableDefinitionTree,
) -> Result<(), SemanticValidationError> {
    #[derive(Deserialize)]
    struct Deployment {
        deployment: serde_json::Value,
    }

    let Deployment { deployment } = serde_saphyr::from_slice(raw_agent_type_content)?;

    let declared_variables: BTreeSet<String> = variables.clone().flatten().into_keys().collect();
    let referenced_variables = referenced_variable_names(&deployment);

    let undeclared_variables: BTreeSet<String> = referenced_variables
        .difference(&declared_variables)
        .cloned()
        .collect();

    if undeclared_variables.is_empty() {
        return Ok(());
    }

    Err(SemanticValidationError::UndeclaredVariables(
        undeclared_variables,
    ))
}

/// Collects the set of `nr-var` names referenced anywhere in `deployment`, ignoring other
/// namespaces (`nr-env`, `nr-sub`, `nr-ac`, ...) and any pipe function suffix.
fn referenced_variable_names(deployment: &serde_json::Value) -> BTreeSet<String> {
    let prefix = format!("{}{}", Namespace::Variable, Namespace::PREFIX_NS_SEPARATOR);
    let deployment_text = deployment.to_string();

    template_re()
        .captures_iter(&deployment_text)
        .filter_map(|captures| {
            let (_, [captured_var, _functions]) = captures.extract();
            captured_var
                .strip_prefix(prefix.as_str())
                .map(str::to_string)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::definition::AgentTypeDefinition;
    use assert_matches::assert_matches;
    use rstest::rstest;

    fn parse(yaml: &str) -> (Vec<u8>, VariableDefinitionTree) {
        let content = yaml.as_bytes().to_vec();
        let definition =
            AgentTypeDefinition::from_slice(&content).expect("fixture yaml must be schema-valid");
        (content, definition.variables)
    }
    const HOST_CONSISTENT_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
protocol_version: "1.0"
platform: host
operating_system: linux
variables:
  bin:
    description: "binary path"
    type: string
    required: true
deployment:
  executables:
    - id: fake_binary
      path: ${nr-var:bin}/fake_binary
"#;

    const K8S_CONSISTENT_NESTED_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
protocol_version: "1.0"
platform: kubernetes
variables:
  container:
    image:
      description: "container image"
      type: string
      required: true
deployment:
  objects:
    cr1:
      apiVersion: fake.group/v1beta1
      kind: FakeKind
      metadata:
        name: fake-object
        namespace: fake-object-namespace
      spec:
        image: ${nr-var:container.image}
"#;

    const HOST_UNDECLARED_VARS_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
protocol_version: "1.0"
platform: host
operating_system: linux
variables: {}
deployment:
  executables:
    - id: fake_binary
      path: ${nr-var:foo}/${nr-var:bar}
"#;

    const HOST_UNUSED_VARIABLE_IS_ALLOWED_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
protocol_version: "1.0"
platform: host
operating_system: linux
variables:
  bin:
    description: "binary path"
    type: string
    required: true
deployment:
  executables:
    - id: fake_binary
      path: /usr/bin/fake_binary
"#;

    const HOST_IGNORES_OTHER_NAMESPACES_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
protocol_version: "1.0"
platform: host
operating_system: linux
variables:
  bin:
    description: "binary path"
    type: string
    required: true
deployment:
  executables:
    - id: fake_binary
      path: ${nr-var:bin}/fake_binary
      args:
        - ${nr-sub:packages.fake_binary.dir}
        - ${nr-env:MY_VAR}
"#;

    const HOST_WITH_PIPE_FUNCTION_YAML: &str = r#"
namespace: newrelic
name: test
version: 0.0.1
protocol_version: "1.0"
platform: host
operating_system: linux
variables:
  bin:
    description: "binary path"
    type: string
    required: true
deployment:
  executables:
    - id: fake_binary
      path: ${nr-var:bin|indent 2}
"#;

    #[rstest]
    #[case::all_consistent_host(HOST_CONSISTENT_YAML, &[])]
    #[case::all_consistent_nested_k8s(K8S_CONSISTENT_NESTED_YAML, &[])]
    #[case::undeclared_variable(HOST_UNDECLARED_VARS_YAML, &["foo","bar"])]
    #[case::unused_declared_variable_is_allowed(HOST_UNUSED_VARIABLE_IS_ALLOWED_YAML, &[])]
    #[case::ignores_other_namespaces(HOST_IGNORES_OTHER_NAMESPACES_YAML, &[])]
    #[case::ignores_pipe_function_suffix(HOST_WITH_PIPE_FUNCTION_YAML, &[])]
    fn test_validate(#[case] yaml: &str, #[case] expected_undeclared: &[&str]) {
        let (content, variables) = parse(yaml);

        let result = validate_variable_references(&content, &variables);

        if expected_undeclared.is_empty() {
            assert_matches!(result, Ok(()));
            return;
        }

        let expected: BTreeSet<String> =
            expected_undeclared.iter().map(|s| s.to_string()).collect();
        match result {
            Err(SemanticValidationError::UndeclaredVariables(undeclared_variables)) => {
                assert_eq!(undeclared_variables, expected);
            }
            other => panic!("expected UndeclaredVariables error, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_reports_deserialize_error_for_malformed_content() {
        let result = validate_variable_references(
            b"deployment: [unclosed",
            &VariableDefinitionTree::default(),
        );

        assert_matches!(result, Err(SemanticValidationError::Deserialize(_)));
    }
}
