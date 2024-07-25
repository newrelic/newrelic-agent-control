use crate::agent_type::variable::definition::VariableDefinition;
use crate::agent_type::variable::namespace::Namespace;
use std::collections::HashMap;
use std::env;

pub fn retrieve_env_var_variables() -> HashMap<String, VariableDefinition> {
    let mut vars: HashMap<String, VariableDefinition> = HashMap::new();
    env::vars().for_each(|(k, v)| {
        vars.insert(
            Namespace::EnvironmentVariable.namespaced_name(k.to_lowercase().as_str()),
            VariableDefinition::new_final_string_variable(v),
        );
    });

    vars
}
