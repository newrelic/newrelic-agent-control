//! Semantic validation for agent type definitions.
//!
//! Checks performed:
//! - **Undeclared variable**: every `${nr-var:X}` reference found anywhere in
//!   `deployment` must have a matching declaration in `variables`.
//! - **Unknown namespace**: every `${nr-xxx:...}` reference found anywhere in `deployment`
//!   must use one of the namespaces in [`Namespace::ALL`].
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
    #[error(
        "unknown namespace(s) referenced in deployment: {}",
        .0.iter().map(String::as_str).collect::<Vec<_>>().join(", ")
    )]
    UnknownNamespaces(BTreeSet<String>),
}

/// A `${nr-xxx:key}` reference found in a deployment's template text.
struct VariableReference<'a> {
    namespace: &'a str,
    key: &'a str,
}

/// Validates that every `${nr-var:X}` reference in `deployment` has a matching declaration in
/// `variables`, and that every `${nr-xxx:...}` reference uses a known namespace.
pub fn validate_variable_references(
    raw_agent_type_content: &[u8],
    variables: &VariableDefinitionTree,
) -> Result<(), SemanticValidationError> {
    #[derive(Deserialize)]
    struct Deployment {
        deployment: serde_json::Value,
    }

    let Deployment { deployment } = serde_saphyr::from_slice(raw_agent_type_content)?;
    let deployment_text = deployment.to_string();
    let references = referenced_variables(&deployment_text);

    let unknown_namespaces = unknown_namespaces(&references);
    if !unknown_namespaces.is_empty() {
        return Err(SemanticValidationError::UnknownNamespaces(
            unknown_namespaces,
        ));
    }

    let declared_variables: BTreeSet<String> = variables.clone().flatten().into_keys().collect();
    let undeclared_variables = undeclared_variables(&references, &declared_variables);
    if !undeclared_variables.is_empty() {
        return Err(SemanticValidationError::UndeclaredVariables(
            undeclared_variables,
        ));
    }

    Ok(())
}

/// Collects every `${nr-xxx:key}` reference found anywhere in `deployment_text`, ignoring any
/// pipe function suffix.
fn referenced_variables(deployment_text: &str) -> Vec<VariableReference<'_>> {
    template_re()
        .captures_iter(deployment_text)
        .filter_map(|captures| {
            let (_, [captured_var, _functions]) = captures.extract();
            captured_var
                .split_once(Namespace::PREFIX_NS_SEPARATOR)
                .map(|(namespace, key)| VariableReference { namespace, key })
        })
        .collect()
}

/// Returns the set of namespace prefixes (e.g. `nr-bogus`) among `references` that don't match
/// any [`Namespace::ALL`] variant.
fn unknown_namespaces(references: &[VariableReference]) -> BTreeSet<String> {
    let known_namespaces: BTreeSet<String> =
        Namespace::ALL.iter().map(ToString::to_string).collect();

    references
        .iter()
        .map(|reference| reference.namespace)
        .filter(|namespace| !known_namespaces.contains(*namespace))
        .map(str::to_string)
        .collect()
}

/// Returns the set of `nr-var` keys among `references` that have no matching entry in
/// `declared_variables`.
fn undeclared_variables(
    references: &[VariableReference],
    declared_variables: &BTreeSet<String>,
) -> BTreeSet<String> {
    let variable_namespace = Namespace::Variable.to_string();

    references
        .iter()
        .filter(|reference| reference.namespace == variable_namespace)
        .map(|reference| reference.key)
        .filter(|key| !declared_variables.contains(*key))
        .map(str::to_string)
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

    const HOST_UNKNOWN_NAMESPACE_YAML: &str = r#"
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
      path: ${nr-bogus:foo}/fake_binary
"#;

    const HOST_UNDECLARED_VAR_AND_UNKNOWN_NAMESPACE_YAML: &str = r#"
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
      path: ${nr-var:foo}/${nr-bogus:bar}
"#;

    #[rstest]
    #[case::all_consistent_host(HOST_CONSISTENT_YAML, &[], &[])]
    #[case::all_consistent_nested_k8s(K8S_CONSISTENT_NESTED_YAML, &[], &[])]
    #[case::undeclared_variable(HOST_UNDECLARED_VARS_YAML, &["foo", "bar"], &[])]
    #[case::unused_declared_variable_is_allowed(HOST_UNUSED_VARIABLE_IS_ALLOWED_YAML, &[], &[])]
    #[case::ignores_other_namespaces(HOST_IGNORES_OTHER_NAMESPACES_YAML, &[], &[])]
    #[case::ignores_pipe_function_suffix(HOST_WITH_PIPE_FUNCTION_YAML, &[], &[])]
    #[case::unknown_namespace(HOST_UNKNOWN_NAMESPACE_YAML, &[], &["nr-bogus"])]
    #[case::unknown_namespace_takes_priority_over_undeclared_variable(
        HOST_UNDECLARED_VAR_AND_UNKNOWN_NAMESPACE_YAML,
        &[],
        &["nr-bogus"]
    )]
    fn test_validate(
        #[case] yaml: &str,
        #[case] expected_undeclared: &[&str],
        #[case] expected_unknown_namespaces: &[&str],
    ) {
        let (content, variables) = parse(yaml);

        let result = validate_variable_references(&content, &variables);

        if !expected_unknown_namespaces.is_empty() {
            let expected: BTreeSet<String> = expected_unknown_namespaces
                .iter()
                .map(|s| s.to_string())
                .collect();
            match result {
                Err(SemanticValidationError::UnknownNamespaces(unknown_namespaces)) => {
                    assert_eq!(unknown_namespaces, expected);
                }
                other => panic!("expected UnknownNamespaces error, got {other:?}"),
            }
            return;
        }

        if !expected_undeclared.is_empty() {
            let expected: BTreeSet<String> =
                expected_undeclared.iter().map(|s| s.to_string()).collect();
            match result {
                Err(SemanticValidationError::UndeclaredVariables(undeclared_variables)) => {
                    assert_eq!(undeclared_variables, expected);
                }
                other => panic!("expected UndeclaredVariables error, got {other:?}"),
            }
            return;
        }

        assert_matches!(result, Ok(()));
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
