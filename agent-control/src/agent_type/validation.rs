//! Validation for agent type definitions: schema and semantics.
use std::collections::BTreeSet;

use thiserror::Error;

use crate::agent_type::definition::{AgentTypeDefinition, AgentTypeDefinitionParseError};
use crate::agent_type::templates::template_re;
use crate::agent_type::variable::namespace::Namespace;
use strum::IntoEnumIterator;

#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("invalid agent type yaml: {0}")]
    Deserialize(#[from] serde_saphyr::Error),
    #[error("invalid agent type definition: {0}")]
    Definition(#[from] AgentTypeDefinitionParseError),
    #[error(
        "undeclared variable(s) referenced in deployment: {}",
        .0.iter().map(String::as_str).collect::<Vec<_>>().join(", ")
    )]
    UndeclaredVariables(BTreeSet<String>),
    #[error(
        "unknown variable namespace(s) referenced in deployment: {}",
        .0.iter().map(String::as_str).collect::<Vec<_>>().join(", ")
    )]
    UnknownNamespaces(BTreeSet<String>),
}

/// A `${nr-xxx:key}` reference found in a deployment's template text.
struct VariableReference<'a> {
    namespace: &'a str,
    key: &'a str,
}

/// Validates Agent Type definition from its raw content. Checks performed:
/// - **Schema**: the raw content is valid YAML and matches the [`AgentTypeDefinition`] shape
///   (required fields, field types, `protocol_version` compatibility).
/// - **Undeclared variable**: every `${nr-var:X}` reference found anywhere in
///   `deployment` must have a matching declaration in `variables`.
/// - **Unknown namespace**: every `${nr-xxx:...}` reference found anywhere in `deployment`
///   must use one of the [`Namespace`] variants.
pub fn validate(raw_agent_type_content: &[u8]) -> Result<(), ValidationError> {
    let document: serde_json::Value = serde_saphyr::from_slice(raw_agent_type_content)?;
    let definition = AgentTypeDefinition::from_value(document.clone())?;

    let deployment_text = document
        .get("deployment")
        .unwrap_or(&serde_json::Value::Null)
        .to_string();
    let references = referenced_variables(&deployment_text);

    let unknown_namespaces = unknown_namespaces(&references);
    if !unknown_namespaces.is_empty() {
        return Err(ValidationError::UnknownNamespaces(unknown_namespaces));
    }

    let declared_variables: BTreeSet<String> = definition.variables.flatten().into_keys().collect();
    let undeclared_variables = undeclared_variables(&references, &declared_variables);
    if !undeclared_variables.is_empty() {
        return Err(ValidationError::UndeclaredVariables(undeclared_variables));
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
/// any [`Namespace`] variant.
fn unknown_namespaces(references: &[VariableReference]) -> BTreeSet<String> {
    let known_namespaces: BTreeSet<String> = Namespace::iter().map(|ns| ns.to_string()).collect();

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
    use assert_matches::assert_matches;
    use rstest::rstest;

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

    const K8S_UNDECLARED_VARIABLE_YAML: &str = r#"
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
    #[case::valid_host(VALID_HOST_YAML)]
    #[case::valid_k8s(VALID_K8S_YAML)]
    #[case::valid_k8s_with_unused_variable(VALID_K8S_WITH_UNUSED_VARIABLE_YAML)]
    #[case::host_consistent(HOST_CONSISTENT_YAML)]
    #[case::k8s_consistent_nested(K8S_CONSISTENT_NESTED_YAML)]
    #[case::host_unused_variable_is_allowed(HOST_UNUSED_VARIABLE_IS_ALLOWED_YAML)]
    #[case::host_ignores_other_namespaces(HOST_IGNORES_OTHER_NAMESPACES_YAML)]
    #[case::host_ignores_pipe_function_suffix(HOST_WITH_PIPE_FUNCTION_YAML)]
    fn test_validate_accepts_valid_definitions(#[case] yaml: &str) {
        assert_matches!(validate(yaml.as_bytes()), Ok(_));
    }

    enum ExpectedError {
        Deserialize,
        Definition,
        UndeclaredVariables(&'static [&'static str]),
        UnknownNamespaces(&'static [&'static str]),
    }

    #[rstest]
    #[case::missing_field(MISSING_REQUIRED_FIELD_YAML, ExpectedError::Definition)]
    #[case::malformed_yaml(MALFORMED_YAML, ExpectedError::Deserialize)]
    #[case::missing_protocol_version(MISSING_PROTOCOL_VERSION_YAML, ExpectedError::Definition)]
    #[case::too_new_protocol_version(TOO_NEW_PROTOCOL_VERSION_YAML, ExpectedError::Definition)]
    #[case::undeclared_variable(
        HOST_UNDECLARED_VARS_YAML,
        ExpectedError::UndeclaredVariables(&["foo", "bar"])
    )]
    #[case::k8s_undeclared_variable(
        K8S_UNDECLARED_VARIABLE_YAML,
        ExpectedError::UndeclaredVariables(&["image"])
    )]
    #[case::unknown_namespace(
        HOST_UNKNOWN_NAMESPACE_YAML,
        ExpectedError::UnknownNamespaces(&["nr-bogus"])
    )]
    #[case::unknown_namespace_takes_priority_over_undeclared_variable(
        HOST_UNDECLARED_VAR_AND_UNKNOWN_NAMESPACE_YAML,
        ExpectedError::UnknownNamespaces(&["nr-bogus"])
    )]
    fn test_validate_rejects_invalid_definitions(
        #[case] yaml: &str,
        #[case] expected: ExpectedError,
    ) {
        let result = validate(yaml.as_bytes());

        match (result, expected) {
            (Err(ValidationError::Deserialize(_)), ExpectedError::Deserialize) => {}
            (Err(ValidationError::Definition(_)), ExpectedError::Definition) => {}
            (
                Err(ValidationError::UndeclaredVariables(actual)),
                ExpectedError::UndeclaredVariables(expected),
            ) => {
                let expected: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
                assert_eq!(actual, expected);
            }
            (
                Err(ValidationError::UnknownNamespaces(actual)),
                ExpectedError::UnknownNamespaces(expected),
            ) => {
                let expected: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
                assert_eq!(actual, expected);
            }
            (other, _) => panic!("unexpected result: {other:?}"),
        }
    }
}
