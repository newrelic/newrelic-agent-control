use std::{
    collections::{HashMap, HashSet},
    env,
};

use anyhow::Result;

use crate::agent_type::{
    templates::template_re,
    variable::{Variable, namespace::Namespace},
};

/// Represents a collection of runtime variables extracted from a sub-agent configuration.
///
/// It will contain something like:
/// ```example
/// {
///     nr-env: {
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
pub struct RuntimeVariables {
    variables: HashMap<String, HashSet<String>>,
}

impl RuntimeVariables {
    /// Extracts runtime variables from a configuration string.
    pub fn from_config(s: &str) -> Self {
        let mut result = RuntimeVariables {
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

            if Namespace::is_runtime_variable(captured_var) {
                result.add_namespaced_variable(captured_var);
            }
        }

        result
    }

    fn add_namespaced_variable(&mut self, variable: &str) {
        let (prefix, var_name) = variable
            .split_once(Namespace::PREFIX_NS_SEPARATOR)
            .map(|v| (v.0.to_string(), v.1.to_string()))
            .expect("Namespace format should be valid");
        self.variables.entry(prefix).or_default().insert(var_name);
    }

    /// Loads environment variables from the runtime variables.
    pub fn load_env_vars(&self) -> Result<HashMap<String, Variable>> {
        let Some(keys) = self
            .variables
            .get(Namespace::EnvironmentVariable.to_string().as_str())
        else {
            return Ok(HashMap::new());
        };

        let mut result = HashMap::new();

        for key in keys {
            let value = env::var_os(key);
            let Some(value) = value else {
                return Err(anyhow::anyhow!("Environment variable '{}' not found", key));
            };

            result.insert(
                Namespace::EnvironmentVariable.namespaced_name(key.as_str()),
                Variable::new_final_string_variable(value.to_string_lossy().to_string()),
            );
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn test_extract_runtime_variables() {
        let input = r#"
data: ${nr-var:var.name|indent 2}
path:${nr-env:PATH_A|indent 2|indent 2}
value: hardcoded value, another_path: ${nr-env:PATH_B}
${nr-env:PATH_C}
eof"#;

        let expected = HashMap::from([(
            "nr-env".to_string(),
            HashSet::from([
                "PATH_A".to_string(),
                "PATH_B".to_string(),
                "PATH_C".to_string(),
            ]),
        )]);
        assert_eq!(RuntimeVariables::from_config(input).variables, expected);
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
        assert_eq!(
            RuntimeVariables::from_config(input).variables,
            HashMap::new()
        );
    }

    #[test]
    fn test_load_env_vars() {
        unsafe { env::set_var("LOAD_ENV_VARS_A", "value_of_LOAD_ENV_VARS_A") };
        unsafe { env::set_var("LOAD_ENV_VARS_B", "value_of_LOAD_ENV_VARS_B") };

        let runtime_variables = RuntimeVariables {
            variables: HashMap::from([(
                "nr-env".to_string(),
                HashSet::from(["LOAD_ENV_VARS_A".to_string(), "LOAD_ENV_VARS_B".to_string()]),
            )]),
        };
        let result = runtime_variables.load_env_vars().unwrap();

        let expected = HashMap::from([
            (
                "nr-env:LOAD_ENV_VARS_A".to_string(),
                Variable::new_final_string_variable("value_of_LOAD_ENV_VARS_A".to_string()),
            ),
            (
                "nr-env:LOAD_ENV_VARS_B".to_string(),
                Variable::new_final_string_variable("value_of_LOAD_ENV_VARS_B".to_string()),
            ),
        ]);
        assert_eq!(result, expected);

        unsafe { env::remove_var("LOAD_ENV_VARS_A") };
        unsafe { env::remove_var("LOAD_ENV_VARS_B") };
    }
}
