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
    pub deployment: Deployment,
}

/// Deployment of an agent type. Each variant carries the shape-specific config for that
/// target. The host variant doesn't carry the operating system — that lives on
/// [crate::agent_type::agent_type_id::AgentTypeID].
///
/// `Deployment` is intentionally **not** `Deserialize` itself: the right variant cannot be
/// chosen from the deployment block alone (host and k8s share field names like `health`).
/// Instead, [crate::agent_type::definition::AgentTypeDefinition]'s custom Deserialize reads
/// the `platform` field from the agent-type id and dispatches the deployment block to either
/// [OnHost] or [K8s] accordingly.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Deployment {
    Host(OnHost),
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
