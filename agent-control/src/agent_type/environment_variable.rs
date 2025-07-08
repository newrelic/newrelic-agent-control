use crate::agent_type::variable::Variable;
use crate::agent_type::variable::namespace::Namespace;
use std::collections::HashMap;
use std::env;

pub fn retrieve_env_var_variables() -> HashMap<String, Variable> {
    let mut vars: HashMap<String, Variable> = HashMap::new();
    env::vars_os().for_each(|(k, v)| {
        vars.insert(
            Namespace::EnvironmentVariable.namespaced_name(k.to_string_lossy().as_ref()),
            Variable::new_final_string_variable(v.to_string_lossy().to_string()),
        );
    });

    vars
}
