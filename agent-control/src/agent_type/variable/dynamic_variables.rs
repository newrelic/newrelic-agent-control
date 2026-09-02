//! Extraction and loading of dynamic variables referenced from a sub-agent configuration.
use std::collections::{HashMap, HashSet};

use crate::agent_type::definition::Variables;
use crate::{
    agent_type::{
        templates::template_re,
        variable::namespace::{Namespace, VariableName},
        variable_value::VariableValue,
    },
    value_provider::{Registry, ValueProvider},
};

/// Represents the prefix used for namespaced variables.
/// Example: "nr-vault", "nr-var", etc.
type NamespacePrefix = String;

/// Represents a collection of variable names for a specific namespace.
/// Example: {"PATH_A", "PATH_B", "sourceA:kv:secrets:password"}.
type VariablesNamesCollection = HashSet<String>;

/// Represents a collection of dynamic variables extracted from a sub-agent configuration.
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
pub struct DynamicVariables {
    variables: HashMap<NamespacePrefix, VariablesNamesCollection>,
}

impl From<&str> for DynamicVariables {
    fn from(s: &str) -> Self {
        let mut result = DynamicVariables {
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

            if Namespace::is_dynamic_variable(captured_var) {
                result.add_namespaced_variable(captured_var);
            }
        }

        result
    }
}

/// Errors produced while extracting or loading dynamic variables.
#[derive(thiserror::Error, Debug)]
pub enum DynamicVariablesError {
    /// A value could not be loaded from its provider.
    #[error("failed to load value {path}: {err_msg}")]
    ValueLoadError {
        /// The path that failed to load.
        path: String,
        /// The underlying error message.
        err_msg: String,
    },

    /// The YAML config could not be parsed.
    #[error("failed to parse yaml config: {0}")]
    YamlParseError(String),
}

impl DynamicVariables {
    /// Loads values from all providers.
    pub fn load_values<S: ValueProvider>(
        &self,
        value_providers_registry: &Registry<S>,
    ) -> Result<Variables, DynamicVariablesError> {
        if value_providers_registry.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::new();
        for (namespace, provider) in value_providers_registry {
            let Some(value_paths) = self.variables.get(&namespace.to_string()) else {
                continue;
            };

            for value_path in value_paths {
                let value = provider.get_value(value_path).map_err(|e| {
                    DynamicVariablesError::ValueLoadError {
                        path: value_path.to_string(),
                        err_msg: e.to_string(),
                    }
                })?;
                result.insert(
                    VariableName::new(*namespace, value_path),
                    VariableValue::String(value),
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
pub fn load_env_vars() -> Variables {
    std::env::vars_os()
        .map(|(k, v)| {
            (
                VariableName::new(Namespace::EnvironmentVariable, k.to_string_lossy()),
                VariableValue::String(v.to_string_lossy().to_string()),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use mockall::predicate;
    use rstest::rstest;
    use std::collections::HashSet;

    use crate::value_provider::{Registry, ValueProviders, vault::tests::MockVault};

    use super::*;

    #[test]
    fn test_extract_dynamic_variables() {
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
        assert_eq!(DynamicVariables::from(input).variables, expected);
    }

    #[rstest]
    fn test_extract_dynamic_variables_when_none_present_in_string(
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
        assert!(DynamicVariables::from(input).variables.is_empty());
    }

    #[test]
    fn test_load_values() {
        let variables = DynamicVariables {
            variables: HashMap::from([(
                "nr-vault".to_string(),
                HashSet::from(["sourceA:my_database:admin/credentials:username".to_string()]),
            )]),
        };

        let mut mock_vault = MockVault::new();
        mock_vault
            .expect_get_value()
            .with(predicate::eq(
                "sourceA:my_database:admin/credentials:username",
            ))
            .returning(|_| Ok("mocked_value_D".to_string()));

        let registry = Registry::from(HashMap::from_iter(vec![(Namespace::Vault, mock_vault)]));
        let result = variables.load_values(&registry).unwrap();
        assert_eq!(
            result,
            HashMap::from([(
                VariableName::new(
                    Namespace::Vault,
                    "sourceA:my_database:admin/credentials:username"
                ),
                VariableValue::String("mocked_value_D".to_string())
            )])
        );
    }

    #[test]
    fn test_load_values_with_empty_registry() {
        let variables = DynamicVariables {
            variables: HashMap::new(),
        };
        let result = variables.load_values(&ValueProviders::default()).unwrap();
        assert!(result.is_empty());
    }
}
