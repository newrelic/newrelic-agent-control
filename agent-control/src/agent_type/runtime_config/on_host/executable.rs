use std::collections::HashMap;

use serde::Deserialize;

use crate::agent_type::{
    definition::Variables,
    error::AgentTypeError,
    runtime_config::{restart_policy::RestartPolicyConfig, templateable_value::TemplateableValue},
    templates::Templateable,
};

pub mod rendered;

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub(super) struct Executable {
    /// Executable identifier for the health checker.
    pub(super) id: String,

    /// Executable binary path. If not an absolute path, the PATH will be searched in an OS-defined way.
    pub(super) path: TemplateableValue<String>, // make it templatable

    /// Arguments passed to the executable.
    #[serde(default)]
    pub(super) args: Args,

    /// Environmental variables passed to the process.
    #[serde(default)]
    pub(super) env: Env,

    /// Defines how the executable will be restarted in case of failure.
    #[serde(default)]
    pub(super) restart_policy: RestartPolicyConfig,
}

impl Templateable for Executable {
    type Output = rendered::Executable;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        let args: Vec<String> = self
            .args
            .0
            .into_iter()
            .map(|arg| arg.template_with(variables))
            .collect::<Result<Vec<String>, AgentTypeError>>()?;

        Ok(Self::Output {
            id: self.id.template_with(variables)?,
            path: self.path.template_with(variables)?,
            args: rendered::Args(args),
            env: self.env.template_with(variables)?,
            restart_policy: self.restart_policy.template_with(variables)?,
        })
    }
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Args(pub Vec<TemplateableValue<String>>);

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Env(pub(super) HashMap<String, TemplateableValue<String>>);

impl Templateable for Env {
    type Output = rendered::Env;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        self.0
            .into_iter()
            .map(|(k, v)| Ok((k, v.template_with(variables)?)))
            .collect::<Result<HashMap<_, _>, _>>()
            .map(rendered::Env)
    }
}
