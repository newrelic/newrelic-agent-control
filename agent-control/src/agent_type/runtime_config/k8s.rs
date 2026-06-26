//! Kubernetes deployment configuration for an agent type: the objects to manage plus health,
//! version and GUID check settings.
use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::guid_config::{GuidCheckerInitialDelay, GuidCheckerInterval};
use crate::agent_type::templates::Templateable;
use crate::agent_type::version_config::{VersionCheckerInitialDelay, VersionCheckerInterval};
use crate::checkers::health::health_checker::{HealthCheckInterval, InitialDelay};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

/// The definition for an K8s supervisor.
///
/// It contains the instructions of what are the agent resources to be managed by the agent-control.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8s {
    /// The Kubernetes objects (usually CRs) to manage, keyed by an arbitrary local name.
    pub objects: HashMap<String, K8sObject>,
    /// Optional health-check configuration.
    pub health: Option<K8sHealthConfig>,
    /// Version-check configuration.
    #[serde(default)]
    pub version: K8sVersionConfig,
    /// GUID-check configuration.
    #[serde(default)]
    pub guid_checker: K8sGuidCheckerConfig,
}

/// A K8s object, usually a CR, to be managed by the agent-control.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8sObject {
    /// The object's `apiVersion`.
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    /// The object's `kind`.
    pub kind: String,
    /// The object's metadata.
    pub metadata: K8sObjectMeta,
    /// Any remaining top-level fields of the object (e.g. `spec`).
    #[serde(default, flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// Metadata for a managed Kubernetes object.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct K8sObjectMeta {
    /// The object's labels.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// The object's name.
    pub name: String,
    /// The object's namespace.
    pub namespace: String,
}

impl Templateable for K8s {
    type Output = Self;
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            objects: self
                .objects
                .into_iter()
                .map(|(k, v)| Ok((k, v.template_with(variables)?)))
                .collect::<Result<HashMap<String, K8sObject>, AgentTypeError>>()?,
            health: self
                .health
                .map(|h| h.template_with(variables))
                .transpose()?,
            version: self.version,
            guid_checker: self.guid_checker,
        })
    }
}

impl Templateable for K8sObject {
    type Output = Self;
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            api_version: self.api_version.clone(),
            kind: self.kind.clone(),
            metadata: self.metadata.template_with(variables)?,
            fields: self.fields.template_with(variables)?,
        })
    }
}

impl Templateable for K8sObjectMeta {
    type Output = Self;

    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            labels: self
                .labels
                .into_iter()
                .map(|(k, v)| Ok((k.template_with(variables)?, v.template_with(variables)?)))
                .collect::<Result<BTreeMap<String, String>, AgentTypeError>>()?,
            name: self.name.template_with(variables)?,
            namespace: self.namespace.template_with(variables)?,
        })
    }
}

/// The kind of Kubernetes resource to health-check.
#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum K8sHealthResourceKind {
    /// A Kubernetes Deployment.
    Deployment,
    /// A Kubernetes DaemonSet.
    DaemonSet,
    /// A Kubernetes StatefulSet.
    StatefulSet,
    /// A New Relic Instrumentation resource.
    Instrumentation,
    /// A Flux HelmRelease workload.
    HelmReleaseWorkload,
}

/// A single Kubernetes resource to include in health checking.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct K8sHealthCheckDefinition {
    pub(crate) name: String,
    pub(crate) namespace: String,
    pub(crate) kind: K8sHealthResourceKind,
    /// This field allows referencing related resources in a different namespace. Eg:
    /// `HelmRelease -> Deployment`, defaults to `namespace`.
    pub(crate) target_namespace: Option<String>,
}

impl Templateable for K8sHealthCheckDefinition {
    type Output = Self;
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            name: self.name.template_with(variables)?,
            namespace: self.namespace.template_with(variables)?,
            kind: self.kind,
            target_namespace: self
                .target_namespace
                .map(|ns| ns.template_with(variables))
                .transpose()?,
        })
    }
}

/// Health-check configuration for a Kubernetes deployment.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8sHealthConfig {
    /// The duration to wait between health checks.
    #[serde(default)]
    pub(crate) interval: HealthCheckInterval,
    /// The initial delay before the first health check is performed.
    #[serde(default)]
    pub(crate) initial_delay: InitialDelay,
    /// Explicit list of Kubernetes check definitions
    #[serde(default)]
    pub(crate) checks: Vec<K8sHealthCheckDefinition>,
}

impl Templateable for K8sHealthConfig {
    type Output = Self;
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            interval: self.interval,
            initial_delay: self.initial_delay,
            checks: self
                .checks
                .into_iter()
                .map(|r| r.template_with(variables))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

/// Version-check configuration for a Kubernetes deployment.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8sVersionConfig {
    /// The duration to wait between version checks.
    #[serde(default)]
    pub(crate) interval: VersionCheckerInterval,
    /// The initial delay before the first version check is performed.
    #[serde(default)]
    pub(crate) initial_delay: VersionCheckerInitialDelay,
}

/// GUID-check configuration for a Kubernetes deployment.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8sGuidCheckerConfig {
    /// The duration to wait between GUID checks.
    #[serde(default)]
    pub(crate) interval: GuidCheckerInterval,
    /// The initial delay before the first GUID check is performed.
    #[serde(default)]
    pub(crate) initial_delay: GuidCheckerInitialDelay,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::agent_type::definition::Variables;
    use crate::agent_type::runtime_config::k8s::{
        K8s, K8sHealthCheckDefinition, K8sHealthResourceKind,
    };
    use crate::agent_type::templates::Templateable;
    use crate::agent_type::variable::Variable;
    use crate::agent_type::version_config::{VersionCheckerInitialDelay, VersionCheckerInterval};

    const RUNTIME_WITH_K8S_DEPLOYMENT: &str = r#"
objects:
  cr1:
    apiVersion: agent_control.version/v0beta1
    kind: Foo
    metadata:
      name: test
      namespace: test-namespace
    spec:
      anyKey: any-value
  cr2:
    apiVersion: agent_control.version/v0beta1
    kind: Foo2
    metadata:
      name: test
      namespace: test-namespace
    # no additional fields
  cr3:
    apiVersion: agent_control.version/v0beta1
    kind: Foo
    metadata:
      name: test
      namespace: test-namespace
    key: value # no spec field
  cr4:
    apiVersion: agent_control.version/v0beta1
    kind: Foo
    metadata:
      name: test
      namespace: test-namespace
      labels:
        foo: bar
    key: value # no spec field
"#;

    #[test]
    fn test_k8s_object() {
        let k8s: K8s = serde_saphyr::from_str(RUNTIME_WITH_K8S_DEPLOYMENT).unwrap();
        assert_eq!("Foo".to_string(), k8s.objects["cr1"].kind);
        assert_eq!(
            "agent_control.version/v0beta1".to_string(),
            k8s.objects["cr1"].api_version
        );
        assert_eq!(
            &serde_json::Value::String("any-value".into()),
            k8s.objects["cr1"]
                .fields
                .get("spec")
                .unwrap()
                .get("anyKey")
                .unwrap()
        );
        assert_eq!("Foo2".to_string(), k8s.objects["cr2"].kind);
        assert_eq!(serde_json::Map::default(), k8s.objects["cr2"].fields);
        assert_eq!(
            &serde_json::Value::String("value".into()),
            k8s.objects["cr3"].fields.get("key").unwrap()
        );

        assert_eq!(
            "bar",
            &k8s.objects["cr4"].metadata.clone().labels["foo"].clone()
        );

        assert_eq!("test", &k8s.objects["cr4"].metadata.clone().name);
    }

    #[test]
    fn test_template_k8s() {
        let untouched_val = "${nr-var:any} no templated";
        let test_agent_id = "id";
        let test_namespace = "test-namespace";
        let k8s_template: K8s = serde_saphyr::from_str(
            format!(
                r#"
objects:
  cr1:
    apiVersion: {untouched_val}
    kind: {untouched_val}
    metadata:
      name: ${{nr-sub:agent_id}}
      namespace: ${{nr-ac:namespace}}
      labels:
        foo: ${{nr-var:any}}
        ${{nr-var:any}}: bar
    spec: ${{nr-var:any}}
"#
            )
            .as_str(),
        )
        .unwrap();

        let value = "test_value";
        let variables = Variables::from([
            (
                "nr-var:any".to_string(),
                Variable::new_string(String::default(), true, None, Some(value.to_string())),
            ),
            (
                "nr-sub:agent_id".to_string(),
                Variable::new_final_string_variable(test_agent_id.to_string()),
            ),
            (
                "nr-ac:namespace".to_string(),
                Variable::new_final_string_variable(test_namespace.to_string()),
            ),
        ]);

        let k8s = k8s_template.template_with(&variables).unwrap();

        let cr1 = k8s.objects.get("cr1").unwrap().clone();

        // Expect no template applied on these fields.
        assert_eq!(cr1.api_version, untouched_val);
        assert_eq!(cr1.kind, untouched_val);

        // Expect template works on fields.
        assert_eq!(cr1.fields.get("spec").unwrap(), value);

        // Expect template works on name.
        assert_eq!(cr1.metadata.name, test_agent_id);

        // Expect template works on namespace.
        assert_eq!(cr1.metadata.namespace, test_namespace);

        // Expect template works on labels.
        let labels = cr1.metadata.labels;
        assert_eq!(labels.get("foo").unwrap(), value);
        assert_eq!(labels.get(value).unwrap(), "bar");
    }

    #[test]
    fn test_template_k8s_health_resources() {
        let k8s_template: K8s = serde_saphyr::from_str(
            r#"
objects: {}
health:
  interval: 30s
  initial_delay: 10s
  checks:
    - namespace: ${nr-ac:namespace}
      name: ${nr-sub:agent_id}
      kind: HelmReleaseWorkload
      target_namespace: ${nr-ac:namespace_agents}
    - namespace: ${nr-ac:namespace_agents}
      name: ${nr-sub:agent_id}
      kind: Instrumentation
    - namespace: ${nr-ac:namespace_agents}
      name: ${nr-sub:agent_id}
      kind: Deployment
"#,
        )
        .unwrap();

        let variables = Variables::from([
            (
                "nr-sub:agent_id".to_string(),
                Variable::new_final_string_variable("my-agent".to_string()),
            ),
            (
                "nr-ac:namespace".to_string(),
                Variable::new_final_string_variable("newrelic".to_string()),
            ),
            (
                "nr-ac:namespace_agents".to_string(),
                Variable::new_final_string_variable("newrelic-agents".to_string()),
            ),
        ]);

        let k8s = k8s_template.template_with(&variables).unwrap();
        let resources = k8s.health.unwrap().checks;

        let expected = vec![
            K8sHealthCheckDefinition {
                name: "my-agent".to_string(),
                namespace: "newrelic".to_string(),
                kind: K8sHealthResourceKind::HelmReleaseWorkload,
                target_namespace: Some("newrelic-agents".to_string()),
            },
            K8sHealthCheckDefinition {
                name: "my-agent".to_string(),
                namespace: "newrelic-agents".to_string(),
                kind: K8sHealthResourceKind::Instrumentation,
                target_namespace: None,
            },
            K8sHealthCheckDefinition {
                name: "my-agent".to_string(),
                namespace: "newrelic-agents".to_string(),
                kind: K8sHealthResourceKind::Deployment,
                target_namespace: None,
            },
        ];

        assert_eq!(resources, expected);
    }

    #[test]
    fn test_k8s_runtime_config_defaults() {
        let k8s: K8s = serde_saphyr::from_str("objects: {}").unwrap();
        assert!(k8s.objects.is_empty());
        assert!(k8s.health.is_none());
        assert_eq!(
            k8s.version.interval,
            VersionCheckerInterval::from(Duration::from_secs(60))
        );
        assert_eq!(
            k8s.version.initial_delay,
            VersionCheckerInitialDelay::from(Duration::from_secs(30))
        );
    }
}
