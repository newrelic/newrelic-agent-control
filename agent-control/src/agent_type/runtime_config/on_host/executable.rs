use std::collections::HashMap;

use serde::Deserialize;

use crate::agent_type::{
    definition::Variables,
    error::AgentTypeError,
    runtime_config::{restart_policy::RestartPolicyConfig, templateable_value::TemplateableValue},
    templates::Templateable,
};

/* FIXME: This is not TEMPLATEABLE for the moment, we need to think what would be the strategy here and clarify:

1. If we perform replacement with the template but the values are not of the expected type, what happens?
2. Should we use an intermediate type with all the end nodes as `String` so we can perform the replacement?
- Add a sanitize or a fallible conversion from the raw intermediate type into into the end type?
*/
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Executable {
    /// Executable binary path. If not an absolute path, the PATH will be searched in an OS-defined way.
    pub path: TemplateableValue<String>, // make it templatable

    /// Arguments passed to the executable.
    #[serde(default)]
    pub args: TemplateableValue<Args>, // make it templatable, it should be aware of the value type, if templated with array, should be expanded

    /// Environmental variables passed to the process.
    #[serde(default)]
    pub env: Env,

    /// Defines how the executable will be restarted in case of failure.
    #[serde(default)]
    pub restart_policy: RestartPolicyConfig,
}

impl Templateable for Executable {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            path: self.path.template_with(variables)?,
            args: self.args.template_with(variables)?,
            env: self.env.template_with(variables)?,
            restart_policy: self.restart_policy.template_with(variables)?,
        })
    }
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Args(pub String);

impl Args {
    pub fn into_vector(self) -> Vec<String> {
        self.0.split_whitespace().map(|s| s.to_string()).collect()
    }
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Env(pub(super) HashMap<String, TemplateableValue<String>>);

impl Env {
    pub fn get(self) -> HashMap<String, String> {
        self.0.into_iter().map(|(k, v)| (k, v.get())).collect()
    }
}

impl Templateable for Env {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        self.0
            .into_iter()
            .map(|(k, v)| Ok((k, v.template_with(variables)?)))
            .collect::<Result<HashMap<_, _>, _>>()
            .map(Env)
    }
}
