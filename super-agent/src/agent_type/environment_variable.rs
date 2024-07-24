use crate::agent_type::variable::definition::VariableDefinition;
use crate::agent_type::variable::namespace::Namespace;
use config::{Config, Environment};
use std::collections::HashMap;

pub fn retrieve_env_var_variables() -> HashMap<String, VariableDefinition> {
    let config_builder = Config::builder()
        .add_source(Environment::default())
        .build()
        .unwrap()
        .try_deserialize::<HashMap<String, String>>()
        .unwrap();

    let mut vars: HashMap<String, VariableDefinition> = HashMap::new();
    config_builder.into_iter().for_each(|(k, v)| {
        vars.insert(
            Namespace::EnvironmentVariable.namespaced_name(k.to_lowercase().as_str()),
            VariableDefinition::new_final_string_variable(v),
        );
    });

    vars
}
