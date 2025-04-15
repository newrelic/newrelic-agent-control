//! # Runtime config module
//!
//! This module defines the runtime configuration for agents, including their deployment details
//! and associated configurations. It provides structures and implementations for deserializing
//! and templating runtime configurations, ensuring that the deployment information is valid and
//! complete.
pub mod health_config;
pub mod k8s;
pub mod onhost;
pub mod restart_policy;
pub mod templateable_value;

use super::definition::Variables;
use super::error::AgentTypeError;
use super::templates::Templateable;
use duration_str::deserialize_duration;
use k8s::K8s;
use onhost::OnHost;
use serde::Deserialize;
use std::time::Duration;
use wrapper_with_default::WrapperWithDefault;

const DEFAULT_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// Strict structure that describes how to start a given agent with all needed binaries, arguments, env, etc.
#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub deployment: Deployment,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Deployment {
    pub on_host: Option<OnHost>,
    pub k8s: Option<K8s>,
}

impl<'de> Deserialize<'de> for Deployment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct DeploymentInner {
            #[serde(default)]
            on_host: Option<OnHost>,
            #[serde(default)]
            k8s: Option<K8s>,
        }
        // Deployment cannot have both fields empty
        let DeploymentInner { on_host, k8s } = DeploymentInner::deserialize(deserializer)?;

        if on_host.is_none() && k8s.is_none() {
            Err(serde::de::Error::custom(
                "field `deployment` must have at least one of the fields `on_host` or `k8s`",
            ))
        } else {
            Ok(Deployment { on_host, k8s })
        }
    }
}

impl Templateable for Deployment {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        /*
        `self.on_host` has type `Option<OnHost>`

        let t = self.on_host.map(|o| o.template_with(variables)); `t` has type `Option<Result<OnHost, AgentTypeError>>`

        Let's visit all the possibilities of `t`.
        When I do `t.transpose()`, which takes an Option<Result<_,_>> and returns a Result<Option<_>,_>, this is what happens:

        ```
        match t {
            None => Ok(None),
            Some(Ok(on_host)) => Ok(Some(on_host)),
            Some(Err(e)) => Err(e),
        }
        ```

        In words:
        - None will be mapped to Ok(None).
        - Some(Ok(_)) will be mapped to Ok(Some(_)).
        - Some(Err(_)) will be mapped to Err(_).

        With `?` I get rid of the original Result<_,_> wrapper type and get the Option<_> (or else the error bubbles up if it contained the Err(_) variant). Then I am able to store that Option<_>, be it None or Some(_), back into the Deployment object which contains the Option<_> field.
        */

        let oh = self
            .on_host
            .map(|oh| oh.template_with(variables))
            .transpose()?;
        let k8s = self
            .k8s
            .map(|k8s| k8s.template_with(variables))
            .transpose()?;
        Ok(Self { on_host: oh, k8s })
    }
}

impl Templateable for Runtime {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            deployment: self.deployment.template_with(variables)?,
        })
    }
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_HEALTH_CHECK_INTERVAL)]
pub struct HealthCheckInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_runtime_deserialization() {
        let rtc = serde_yaml::from_str::<Runtime>("deployment: {}");
        assert!(rtc.is_err_and(|e| {
            e.to_string().contains(
                "field `deployment` must have at least one of the fields `on_host` or `k8s`",
            )
        }));

        let rtc = serde_yaml::from_str::<Runtime>("deployment: ");
        assert!(rtc.is_err_and(|e| {
            e.to_string().contains(
                "field `deployment` must have at least one of the fields `on_host` or `k8s`",
            )
        }));

        let rtc = serde_yaml::from_str::<Runtime>("");
        assert!(rtc.is_err_and(|e| e.to_string().contains("missing field `deployment`")));
    }
}
