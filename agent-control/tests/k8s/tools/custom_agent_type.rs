use super::agent_control::DYNAMIC_AGENT_TYPE_FILENAME;
use fs::file::LocalFile;
use fs::file::writer::FileWriter;
use newrelic_agent_control::agent_type::agent_type_id::AgentTypeID;
use newrelic_agent_control::agent_type::definition::AgentTypeDefinition;
use std::fmt::Display;
use std::path::Path;

/// Helper to build a Custom Agent type with defaults ready to use in k8s integration tests.
pub struct K8sCustomAgentType {
    agent_type_id: AgentTypeID,
    variables: Option<serde_json::Value>,
    health: Option<serde_json::Value>,
    objects: Option<serde_json::Value>,
}

impl Default for K8sCustomAgentType {
    fn default() -> Self {
        Self {
            agent_type_id: Self::default_agent_type_id(),
            variables: Some(
                serde_saphyr::from_str(
                    r#"
chart_values:
  description: "chart_values"
  type: yaml
  required: false
  default: { }
"#,
                )
                .unwrap(),
            ),
            health: Some(Self::default_health()),
            objects: Some(Self::default_objects()),
        }
    }
}

impl Display for K8sCustomAgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let content = format!(
            r#"
        namespace: {}
        name: {}
        version: {}
        platform: kubernetes
        protocol_version: "1.0"
        "#,
            self.agent_type_id.namespace(),
            self.agent_type_id.name(),
            self.agent_type_id.version(),
        );
        let mut content: serde_json::Map<String, serde_json::Value> =
            serde_saphyr::from_str(&content).unwrap();

        let variables = self
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

impl K8sCustomAgentType {
    fn default_agent_type_id() -> AgentTypeID {
        AgentTypeID::try_from("newrelic/com.newrelic.custom_agent:0.0.1").unwrap()
    }

    fn default_health() -> serde_json::Value {
        serde_saphyr::from_str(
            r#"
interval: 5s
initial_delay: 2s
checks:
  - namespace: ${nr-ac:namespace}
    name: ${nr-sub:agent_id}
    kind: HelmReleaseWorkload
    target_namespace: ${nr-ac:namespace_agents}
"#,
        )
        .unwrap()
    }

    fn default_objects() -> serde_json::Value {
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
        )
        .unwrap()
    }

    pub fn empty() -> Self {
        Self {
            agent_type_id: Self::default_agent_type_id(),
            variables: None,
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
            variables: Some(serde_saphyr::from_str(variables).unwrap()),
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
            agent_type_id: AgentTypeID::try_from(agent_type_id).unwrap(),
            ..self
        }
    }

    /// Writes the custom agent type to the fixed dynamic agent type file path used by k8s tests.
    pub fn build(self, local_dir: &Path) {
        let agent_type_file_path = local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME);

        let parsed_agent_type = AgentTypeDefinition::from_slice(self.to_string().as_bytes());
        assert!(
            parsed_agent_type.is_ok(),
            "K8sCustomAgentType did not produce valid AgentTypeDefinition: {}\n{}",
            parsed_agent_type.err().unwrap(),
            self
        );

        std::fs::create_dir_all(agent_type_file_path.parent().unwrap()).unwrap();
        LocalFile
            .write(&agent_type_file_path, self.to_string())
            .expect("failed to write custom agent type");
    }
}
