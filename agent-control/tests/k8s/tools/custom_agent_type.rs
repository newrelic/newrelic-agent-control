use crate::common::custom_agent_type::CommonCustomAgentTypeBuilder;
use newrelic_agent_control::agent_type::agent_type_id::AgentTypeID;
use std::fmt::Display;
use std::path::Path;

/// Helper to build a Custom Agent type with defaults ready to use in k8s integration tests.
pub struct K8sCustomAgentTypeBuilder {
    common: CommonCustomAgentTypeBuilder,
    health: Option<serde_json::Value>,
    objects: Option<serde_json::Value>,
}

impl Default for K8sCustomAgentTypeBuilder {
    fn default() -> Self {
        Self::empty()
            .with_variables(
                r#"
chart_values:
  description: "chart_values"
  type: yaml
  required: false
  default: { }
"#,
            )
            .with_health(Some(
                r#"
interval: 5s
initial_delay: 2s
checks:
  - namespace: ${nr-ac:namespace}
    name: ${nr-sub:agent_id}
    kind: HelmReleaseWorkload
    target_namespace: ${nr-ac:namespace_agents}
"#,
            ))
            .with_objects(Some(
                r#"
repository:
  apiVersion: source.toolkit.fluxcd.io/v1
  kind: HelmRepository
  metadata:
    name: ${nr-sub:agent_id}
    namespace: ${nr-ac:namespace}
  spec:
    # we don't want to trigger this in the test to avoid extra load in the cluster
    interval: 99m
    url: https://helm.github.io/examples
release:
  apiVersion: helm.toolkit.fluxcd.io/v2
  kind: HelmRelease
  metadata:
    name: ${nr-sub:agent_id}
    namespace: ${nr-ac:namespace}
  spec:
    # we don't want to trigger this in the test to avoid extra load in the cluster
    interval: 10s
    releaseName: ${nr-sub:agent_id}
    targetNamespace: ${nr-ac:namespace_agents}
    chart:
      spec:
        chart: hello-world
        version: 0.1.0
        sourceRef:
          kind: HelmRepository
          name: ${nr-sub:agent_id}
          namespace: ${nr-ac:namespace}
    install:
      disableWait: true
      disableWaitForJobs: true
      disableTakeOwnership: true
    upgrade:
      disableWait: true
      disableWaitForJobs: true
      disableTakeOwnership: true
      cleanupOnFail: true
    values:
      ${nr-var:chart_values}
"#,
            ))
    }
}

impl Display for K8sCustomAgentTypeBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let content = format!(
            r#"
        namespace: {}
        name: {}
        version: {}
        platform: kubernetes
        protocol_version: "1.0"
        "#,
            self.common.agent_type_id.namespace(),
            self.common.agent_type_id.name(),
            self.common.agent_type_id.version(),
        );
        let mut content: serde_json::Map<String, serde_json::Value> =
            serde_saphyr::from_str(&content).unwrap();

        let variables = self
            .common
            .variables
            .clone()
            .unwrap_or_else(|| serde_json::Map::<String, serde_json::Value>::new().into());

        let mut deployment = serde_json::Map::<String, serde_json::Value>::new();
        if let Some(health) = self.health.as_ref() {
            deployment.insert("health".into(), health.clone());
        }
        deployment.insert(
            "objects".into(),
            self.objects
                .clone()
                .unwrap_or_else(|| serde_json::Map::<String, serde_json::Value>::new().into()),
        );

        content.insert("variables".into(), variables);
        content.insert("deployment".into(), deployment.into());
        let content = serde_json::Value::from(content);

        write!(f, "{}", serde_saphyr::to_string(&content).unwrap())
    }
}

impl K8sCustomAgentTypeBuilder {
    pub fn empty() -> Self {
        Self {
            common: CommonCustomAgentTypeBuilder::new(
                AgentTypeID::try_from("newrelic/com.newrelic.custom_agent:0.0.1").unwrap(),
            ),
            health: None,
            objects: None,
        }
    }

    /// Like [`Self::default`], but the HelmRelease values are provided through a separate Secret
    /// (`valuesFrom`) instead of being inlined in the release spec, exercising the split
    /// namespace/values-secret path.
    pub fn split_ns() -> Self {
        Self {
            objects: Some(
                serde_saphyr::from_str(
                    r#"
repository:
  apiVersion: source.toolkit.fluxcd.io/v1
  kind: HelmRepository
  metadata:
    name: ${nr-sub:agent_id}
    namespace: ${nr-ac:namespace}
  spec:
    # we don't want to trigger this in the test to avoid extra load in the cluster
    interval: 99m
    url: https://helm.github.io/examples
default-values:
  apiVersion: v1
  kind: Secret
  metadata:
    name: default-values-${nr-sub:agent_id}
    namespace: ${nr-ac:namespace}
  stringData:
    values.yaml: |
      ${nr-var:chart_values}
release:
  apiVersion: helm.toolkit.fluxcd.io/v2
  kind: HelmRelease
  metadata:
    name: ${nr-sub:agent_id}
    namespace: ${nr-ac:namespace}
  spec:
    # until we address https://new-relic.atlassian.net/browse/NR-435351
    interval: 10s
    targetNamespace: ${nr-ac:namespace_agents}
    releaseName: ${nr-sub:agent_id}
    chart:
      spec:
        chart: hello-world
        version: 0.1.0
        sourceRef:
          kind: HelmRepository
          name: ${nr-sub:agent_id}
    install:
      disableWait: true
      disableWaitForJobs: true
      disableTakeOwnership: true
    upgrade:
      disableWait: true
      disableWaitForJobs: true
      disableTakeOwnership: true
      cleanupOnFail: true
    valuesFrom:
      - kind: Secret
        name: default-values-${nr-sub:agent_id}
        valuesKey: values.yaml
"#,
                )
                .unwrap(),
            ),
            ..Self::default()
        }
    }

    pub fn with_variables(self, variables: &str) -> Self {
        Self {
            common: self.common.with_variables(variables),
            ..self
        }
    }

    pub fn with_health(self, health: Option<&str>) -> Self {
        Self {
            health: health.map(|h| serde_saphyr::from_str(h).unwrap()),
            ..self
        }
    }

    pub fn with_objects(self, objects: Option<&str>) -> Self {
        Self {
            objects: objects.map(|o| serde_saphyr::from_str(o).unwrap()),
            ..self
        }
    }

    pub fn with_agent_type_id(self, agent_type_id: &str) -> Self {
        Self {
            common: self.common.with_agent_type_id(agent_type_id),
            ..self
        }
    }

    pub fn write(self, local_dir: &Path) -> String {
        self.common.write(local_dir, &self.to_string())
    }
}
