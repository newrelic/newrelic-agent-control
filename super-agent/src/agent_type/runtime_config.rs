use super::{
    definition::TemplateableValue, health_config::OnHostHealthConfig,
    restart_policy::RestartPolicyConfig,
};
use crate::agent_type::health_config::K8sHealthConfig;
use serde::Deserialize;
use std::collections::HashMap;

/// Strict structure that describes how to start a given agent with all needed binaries, arguments, env, etc.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct Runtime {
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
    #[serde(default)]
    pub enable_file_logging: TemplateableValue<bool>,
}

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

    /// Enables and define health checks configuration.
    pub health: Option<OnHostHealthConfig>,
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

/// The definition for an K8s supervisor.
///
/// It contains the instructions of what are the agent resources to be managed by the super-agent.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8s {
    pub objects: HashMap<String, K8sObject>,
    pub health: Option<K8sHealthConfig>,
}

/// A K8s object, usually a CR, to be managed by the super-agent.
// TODO: at lest, the spec should be templatable.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8sObject {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: K8sObjectMeta,
    #[serde(default, flatten)]
    pub fields: serde_yaml::Mapping,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct K8sObjectMeta {
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
    pub name: String,
}

#[cfg(test)]
mod test {
    use super::*;

    const RUNTIME_WITH_K8S_DEPLOYMENT: &str = r#"
deployment:
  k8s:
    objects:
      cr1:
        apiVersion: super_agent.version/v0beta1
        kind: Foo
        metadata:
          name: test
        spec:
          anyKey: any-value
      cr2:
        apiVersion: super_agent.version/v0beta1
        kind: Foo2
        metadata:
          name: test
        # no additional fields
      cr3:
        apiVersion: super_agent.version/v0beta1
        kind: Foo
        metadata:
          name: test
        key: value # no spec field
      cr4:
        apiVersion: super_agent.version/v0beta1
        kind: Foo
        metadata:
          name: test
          labels:
            foo: bar
        key: value # no spec field
"#;

    #[test]
    fn test_k8s_object() {
        let rtc: Runtime = serde_yaml::from_str(RUNTIME_WITH_K8S_DEPLOYMENT).unwrap();
        assert!(rtc.deployment.on_host.is_none());
        let k8s = rtc.deployment.k8s.unwrap();
        assert_eq!("Foo".to_string(), k8s.objects["cr1"].kind);
        assert_eq!(
            "super_agent.version/v0beta1".to_string(),
            k8s.objects["cr1"].api_version
        );
        assert_eq!(
            &serde_yaml::Value::String("any-value".into()),
            k8s.objects["cr1"]
                .fields
                .get("spec")
                .unwrap()
                .get("anyKey")
                .unwrap()
        );
        assert_eq!("Foo2".to_string(), k8s.objects["cr2"].kind);
        assert_eq!(serde_yaml::Mapping::default(), k8s.objects["cr2"].fields);
        assert_eq!(
            &serde_yaml::Value::String("value".into()),
            k8s.objects["cr3"].fields.get("key").unwrap()
        );

        assert_eq!(
            "bar",
            &k8s.objects["cr4"].metadata.clone().labels["foo"].clone()
        );

        assert_eq!("test", &k8s.objects["cr4"].metadata.clone().name);
    }
}
