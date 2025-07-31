use std::collections::{HashMap, HashSet};

use tracing::error;

use crate::{
    agent_type::{
        templates::template_re,
        variable::{Variable, namespace::Namespace},
    },
    secrets_provider::{Registry, SecretsProvider},
    values::yaml_config::YAMLConfig,
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

impl TryFrom<YAMLConfig> for SecretVariables {
    type Error = SecretVariablesError;

    fn try_from(config: YAMLConfig) -> Result<Self, Self::Error> {
        let config: String = config
            .try_into()
            .map_err(|_| SecretVariablesError::YamlParseError)?;
        Ok(SecretVariables::from(config.as_str()))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SecretVariablesError {
    #[error("failed to load secret: {0}")]
    SecretsLoadError(String),

    #[error("failed to parse yaml config")]
    YamlParseError,
}

impl SecretVariables {
    /// Loads secrets from all providers.
    pub fn load_secrets<S: SecretsProvider>(
        &self,
        secrets_providers_registry: &Registry<S>,
    ) -> Result<HashMap<String, Variable>, SecretVariablesError> {
        if secrets_providers_registry.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::new();
        for (namespace, provider) in secrets_providers_registry {
            let Some(secrets_paths) = self.variables.get(&namespace.to_string()) else {
                continue;
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

/// Loads all environment variables present in the system.
pub fn load_env_vars() -> HashMap<String, Variable> {
    std::env::vars_os()
        .map(|(k, v)| {
            (
                Namespace::EnvironmentVariable.namespaced_name(&k.to_string_lossy()),
                Variable::new_final_string_variable(v.to_string_lossy().to_string()),
            )
        })
        .collect::<HashMap<String, Variable>>()
}

#[cfg(test)]
mod tests {
    use mockall::predicate;
    use rstest::rstest;
    use std::collections::HashSet;

    use crate::secrets_provider::{Registry, SecretsProviders, vault::tests::MockVault};

    use super::*;

    #[test]
    fn test_extract_secrets() {
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
    fn test_extract_secrets_when_no_secrets_present_in_string(
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
        assert!(SecretVariables::from(input).variables.is_empty());
    }

    #[test]
    fn test_load_secrets() {
        let secrets = SecretVariables {
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

        let registry = Registry::from(HashMap::from_iter(vec![(Namespace::Vault, mock_vault)]));
        let result = secrets.load_secrets(&registry).unwrap();
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
        let secrets = SecretVariables {
            variables: HashMap::new(),
        };
        let result = secrets.load_secrets(&SecretsProviders::new()).unwrap();
        assert!(result.is_empty());
    }
}
