use std::collections::HashMap;

use serde::Deserialize;

use crate::agent_type::{
    definition::Variables,
    error::AgentTypeError,
    runtime_config::{
        restart_policy::{RenderedRestartPolicyConfig, RestartPolicyConfig},
        templateable_value::TemplateableValue,
    },
    templates::Templateable,
};

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub(super) struct Executable {
    /// Executable identifier for the health checker.
    pub(super) id: String,

    /// Executable binary path. If not an absolute path, the PATH will be searched in an OS-defined way.
    pub(super) path: TemplateableValue<String>, // make it templatable

    /// Arguments passed to the executable.
    #[serde(default)]
    pub(super) args: TemplateableValue<Args>, // make it templatable, it should be aware of the value type, if templated with array, should be expanded

    /// Environmental variables passed to the process.
    #[serde(default)]
    pub(super) env: Env,

    /// Defines how the executable will be restarted in case of failure.
    #[serde(default)]
    pub(super) restart_policy: RestartPolicyConfig,
}

impl Templateable for Executable {
    type Output = RenderedExecutable;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        Ok(Self::Output {
            id: self.id.template_with(variables)?,
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

impl Templateable for Env {
    type Output = RenderedEnv;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        self.0
            .into_iter()
            .map(|(k, v)| Ok((k, v.template_with(variables)?)))
            .collect::<Result<HashMap<_, _>, _>>()
            .map(RenderedEnv)
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct RenderedExecutable {
    /// Executable identifier for the health checker.
    pub id: String,
    /// Executable binary path. If not an absolute path, the PATH will be searched in an OS-defined way.
    pub path: String,
    /// Arguments passed to the executable.
    pub args: Args,
    /// Environmental variables passed to the process.
    pub env: RenderedEnv,
    /// Defines how the executable will be restarted in case of failure.
    pub restart_policy: RenderedRestartPolicyConfig,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct RenderedEnv(pub HashMap<String, String>);
