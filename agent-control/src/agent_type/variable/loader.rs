use std::{collections::HashMap, env};

use anyhow::Result;

use crate::agent_type::variable::{
    definition::VariableDefinition, extractor::RuntimeVariables, namespace::Namespace,
};

pub fn load_env_vars(
    runtime_variables: &RuntimeVariables,
) -> Result<HashMap<String, VariableDefinition>> {
    let mut result = HashMap::new();

    if let Some(keys) = runtime_variables.get(Namespace::EnvironmentVariable.to_string().as_str()) {
        for key in keys {
            let value = env::var_os(key.clone());
            let Some(value) = value else {
                return Err(anyhow::anyhow!("Environment variable '{}' not found", key));
            };

            result.insert(
                Namespace::EnvironmentVariable.namespaced_name(key.as_str()),
                VariableDefinition::new_final_string_variable(value.to_string_lossy().to_string()),
            );
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn test_retrieve_runtime_variables_values() {
        unsafe { env::set_var("PATH_A", "value_of_PATH_A") };
        unsafe { env::set_var("PATH_B", "value_of_PATH_B") };

        let input = HashMap::from([(
            "nr-env".to_string(),
            HashSet::from(["PATH_A".to_string(), "PATH_B".to_string()]),
        )]);
        let result = load_env_vars(&input).unwrap();

        let expected = HashMap::from([
            (
                "nr-env:PATH_A".to_string(),
                VariableDefinition::new_final_string_variable("value_of_PATH_A".to_string()),
            ),
            (
                "nr-env:PATH_B".to_string(),
                VariableDefinition::new_final_string_variable("value_of_PATH_B".to_string()),
            ),
        ]);
        assert_eq!(result, expected);
    }
}
