//! # Runtime config module
//!
//! This module defines the deployment instructions of an agent type and the templating logic
//! that turns the parsed deployment into its rendered form.
pub mod health_config;
pub mod k8s;
pub mod on_host;
pub mod rendered;
pub mod restart_policy;
pub mod templateable_value;
pub mod version_config;

use super::definition::Variables;
use super::error::AgentTypeError;
use super::templates::Templateable;
use k8s::K8s;
use on_host::OnHost;

/// Strict structure that describes how to start a given agent with all needed binaries,
/// arguments, env, etc.
#[derive(Debug, Clone, PartialEq)]
pub struct Runtime {
    /// The deployment instructions for the agent.
    pub deployment: Deployment,
}

/// Deployment of an agent type. Each variant carries the shape-specific config for that
/// target.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Deployment {
    /// An on-host deployment.
    Host(OnHost),
    /// A Kubernetes deployment.
    K8s(K8s),
}

impl Templateable for Deployment {
    type Output = rendered::Deployment;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        match self {
            Deployment::Host(on_host) => Ok(rendered::Deployment::Host(
                on_host.template_with(variables)?,
            )),
            Deployment::K8s(k8s) => Ok(rendered::Deployment::K8s(k8s.template_with(variables)?)),
        }
    }
}

impl Templateable for Runtime {
    type Output = rendered::Runtime;

    fn template_with(self, variables: &Variables) -> Result<Self::Output, AgentTypeError> {
        Ok(Self::Output {
            deployment: self.deployment.template_with(variables)?,
        })
    }
}
