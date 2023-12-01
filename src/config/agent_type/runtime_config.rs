use std::collections::HashMap;

use serde::Deserialize;

use super::{agent_types::TemplateableValue, restart_policy::RestartPolicyConfig};

/// Strict structure that describes how to start a given agent with all needed binaries, arguments, env, etc.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct RuntimeConfig {
    pub deployment: Deployment,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Deployment {
    pub on_host: Option<OnHost>,
    pub k8s: Option<K8s>,
}

/// The definition for an on-host supervisor.
///
/// It contains the instructions of what are the agent binaries, command-line arguments, the environment variables passed to it and the restart policy of the supervisor.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct OnHost {
    pub executables: Vec<Executable>,
}

/* FIXME: This is not TEMPLATEABLE for the moment, we need to think what would be the strategy here and clarify:

1. If we perform replacement with the template but the values are not of the expected type, what happens?
2. Should we use an intermediate type with all the end nodes as `String` so we can perform the replacement?
  - Add a sanitize or a fallible conversion from the raw intermediate type into into the end type?
*/
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Executable {
    pub path: TemplateableValue<String>, // make it templatable
    #[serde(default)]
    pub args: TemplateableValue<Args>, // make it templatable, it should be aware of the value type, if templated with array, should be expanded
    #[serde(default)]
    pub env: TemplateableValue<Env>, // make it templatable, it should be aware of the value type, if templated with array, should be expanded "STAGING=true ${variable_1}" variable_1 : VERBOSE=1
    #[serde(default)]
    pub restart_policy: RestartPolicyConfig,
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Args(pub String);

impl Args {
    pub fn into_vector(self) -> Vec<String> {
        self.0.split_whitespace().map(|s| s.to_string()).collect()
    }
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Env(pub String);

impl Env {
    pub fn into_map(self) -> HashMap<String, String> {
        self.0
            .split_whitespace()
            .map(|s| {
                // FIXME: Non-existing '=' on input??
                s.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .unwrap()
            })
            .collect()
    }
}

/// The definition for an K8s supervisor.
///
/// It contains the instructions of what are the agent resources to be managed by the super-agent.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8s {
    pub objects: HashMap<String, K8sObject>,
}

/// A K8s object, usually a CR, to be managed by the super-agent.
// TODO: at lest, the spec should be templatable.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8sObject {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    #[serde(rename = "kind")]
    pub kind: String,
    #[serde(default)]
    pub spec: serde_yaml::Value,
}
