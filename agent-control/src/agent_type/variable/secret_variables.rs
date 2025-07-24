use std::collections::{HashMap, HashSet};

use tracing::error;

use crate::{
    agent_type::{
        templates::template_re,
        variable::{Variable, namespace::Namespace},
    },
    secrets_provider::{SecretsProvider, SecretsProviderType, SecretsProvidersRegistry},
};

/// Represents the prefix used for namespaced variables.
/// Example: "nr-vault", "nr-var", etc.
type NamespacePrefix = String;

/// Represents a collection of variable names for a specific namespace.
/// Example: {"PATH_A", "PATH_B", "sourceA:kv:secrets:password"}.
type VariablesNamesCollection = HashSet<String>;

/// Represents a collection of secret variables extracted from a sub-agent configuration.
///
/// It will contain something like:
/// ```example
/// {
///     nr-vault: {
///         PATH_A,
///         PATH_B,
///     },
///     nr-other: {
///         VAR_A,
///         VAR_B,
///         VAR_C,
///     },
/// }
/// ```
pub struct SecretVariables {
    variables: HashMap<NamespacePrefix, VariablesNamesCollection>,
}

impl From<&str> for SecretVariables {
    fn from(s: &str) -> Self {
        let mut result = SecretVariables {
            variables: HashMap::new(),
        };

        let re_template = template_re();
        for captures in re_template.captures_iter(s) {
            // "Example with a template: ${nr-var:name|indent 2|to_upper}"
            // templatable_placeholder="${nr-var:name|indent 2|to_upper}"
            // captured_var="nr-var:name"
            // captured_functions="|indent 2|to_upper"
            let (_templatable_placeholder, [captured_var, _captured_functions]) =
                captures.extract();

            if Namespace::is_secret_variable(captured_var) {
                result.add_namespaced_variable(captured_var);
            }
        }

        result
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SecretVariablesError {
    #[error("failed to load secret: {0}")]
    SecretsLoadError(String),
}

impl SecretVariables {
    /// Loads secrets from all providers.
    pub fn load_all_secrets(
        &self,
        secrets_providers_registry: SecretsProvidersRegistry,
    ) -> Result<HashMap<String, Variable>, SecretVariablesError> {
        if secrets_providers_registry.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::new();
        for (namespace, provider) in secrets_providers_registry {
            let secrets_map = match provider {
                SecretsProviderType::Vault(provider) => {
                    self.load_secrets_at(namespace, provider)?
                }
                SecretsProviderType::K8sSecret(provider) => {
                    self.load_secrets_at(namespace, provider)?
                }
            };
            result.extend(secrets_map);
        }

        Ok(result)
    }

    /// Loads secrets from the given provider.
    fn load_secrets_at<SP: SecretsProvider>(
        &self,
        namespace: Namespace,
        provider: SP,
    ) -> Result<HashMap<String, Variable>, SecretVariablesError> {
        let mut result = HashMap::new();
        let Some(secrets_paths) = self.variables.get(&namespace.to_string()) else {
            return Ok(HashMap::new());
        };

        for secret_path in secrets_paths {
            let secret_value = provider
                .get_secret(secret_path)
                .map_err(|_| SecretVariablesError::SecretsLoadError(secret_path.to_string()))
                .inspect_err(|error| {
                    error!("{error}");
                })?;
            result.insert(
                namespace.namespaced_name(secret_path),
                Variable::new_final_string_variable(secret_value),
            );
        }

        Ok(result)
    }

    fn add_namespaced_variable(&mut self, variable: &str) {
        let (prefix, var_name) = variable
            .split_once(Namespace::PREFIX_NS_SEPARATOR)
            .map(|v| (v.0.to_string(), v.1.to_string()))
            .expect("Namespace format should be valid");
        self.variables.entry(prefix).or_default().insert(var_name);
    }
}

#[cfg(test)]
mod tests {
    use mockall::predicate;
    use rstest::rstest;
    use std::collections::HashSet;

    use crate::secrets_provider::vault::tests::MockVault;

    use super::*;

    #[test]
    fn test_extract_runtime_variables() {
        let input = r#"
data: ${nr-var:var.name|indent 2}
path:${nr-vault:PATH_A|indent 2|indent 2}
value: hardcoded value, another_path: ${nr-vault:PATH_B}
${nr-vault:PATH_C}
${nr-vault:PATH_D}
${nr-vault:sourceA:my_database:admin/credentials:username}
eof"#;

        let expected = HashMap::from([(
            "nr-vault".to_string(),
            HashSet::from([
                "PATH_A".to_string(),
                "PATH_B".to_string(),
                "PATH_C".to_string(),
                "PATH_D".to_string(),
                "sourceA:my_database:admin/credentials:username".to_string(),
            ]),
        )]);
        assert_eq!(SecretVariables::from(input).variables, expected);
    }

    #[rstest]
    fn test_extract_runtime_variables_when_no_runtime_variables_are_present(
        #[values(
            "test string",
            "${nr-var:var.name}",
            "${nr-var:var.name|indent 2}",
            "${nr-var:var.name|indent 2|indent 2}",
            "${nr-sub:var.name}",
            "${nr-ac:var.name}",
            "${nr-var:var.name|indent 2} ${nr-var:var.name|indent 2} ${nr-var:var.name|indent 2}"
        )]
        input: &str,
    ) {
        assert_eq!(SecretVariables::from(input).variables, HashMap::new());
    }

    #[test]
    fn test_load_secrets_at() {
        let runtime_variables = SecretVariables {
            variables: HashMap::from([(
                "nr-vault".to_string(),
                HashSet::from(["sourceA:my_database:admin/credentials:username".to_string()]),
            )]),
        };

        let mut mock_vault = MockVault::new();
        mock_vault
            .expect_get_secret()
            .with(predicate::eq(
                "sourceA:my_database:admin/credentials:username",
            ))
            .returning(|_| Ok("mocked_value_D".to_string()));

        let result = runtime_variables
            .load_secrets_at(Namespace::Vault, mock_vault)
            .unwrap();
        assert_eq!(
            result,
            HashMap::from([(
                "nr-vault:sourceA:my_database:admin/credentials:username".to_string(),
                Variable::new_final_string_variable("mocked_value_D".to_string())
            )])
        );
    }

    #[test]
    fn test_load_secrets_with_empty_registry() {
        let runtime_variables = SecretVariables {
            variables: HashMap::new(),
        };
        let result = runtime_variables
            .load_all_secrets(SecretsProvidersRegistry::new())
            .unwrap();
        assert!(result.is_empty());
    }
}
