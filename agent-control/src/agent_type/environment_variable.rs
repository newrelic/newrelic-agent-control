use crate::agent_type::variable::definition::VariableDefinition;
use crate::agent_type::variable::namespace::Namespace;
use std::collections::HashMap;
use std::env;

pub fn retrieve_env_var_variables() -> HashMap<String, VariableDefinition> {
    let mut vars: HashMap<String, VariableDefinition> = HashMap::new();
    // TODO: We can skip this step and do this on the same way as secret providers,
    //  first parse the values file, gather all nr-env and retrieve each of them.

    env::vars_os().for_each(|(k, v)| {
        vars.insert(
            Namespace::EnvironmentVariable.namespaced_name(k.to_string_lossy().as_ref()),
            VariableDefinition::new_final_string_variable(v.to_string_lossy().to_string()),
        );
    });

    vars
}
