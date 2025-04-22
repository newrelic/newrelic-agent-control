use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::runtime_config::HealthCheckInterval;
use crate::agent_type::templates::Templateable;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

/// The definition for an K8s supervisor.
///
/// It contains the instructions of what are the agent resources to be managed by the agent-control.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8s {
    pub objects: HashMap<String, K8sObject>,
    pub health: Option<K8sHealthConfig>,
}

/// A K8s object, usually a CR, to be managed by the agent-control.
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
    pub namespace: String,
}

impl Templateable for K8s {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            objects: self
                .objects
                .into_iter()
                .map(|(k, v)| Ok((k, v.template_with(variables)?)))
                .collect::<Result<HashMap<String, K8sObject>, AgentTypeError>>()?,
            health: self.health,
        })
    }
}

impl Templateable for K8sObject {
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

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8sHealthConfig {
    /// The duration to wait between health checks.
    pub(crate) interval: HealthCheckInterval,
}

#[cfg(test)]
mod tests {
    use crate::agent_type::definition::Variables;
    use crate::agent_type::runtime_config::k8s::K8s;
    use crate::agent_type::templates::Templateable;
    use crate::agent_type::variable::definition::VariableDefinition;

    const RUNTIME_WITH_K8S_DEPLOYMENT: &str = r#"
objects:
  cr1:
    apiVersion: agent_control.version/v0beta1
    kind: Foo
    metadata:
      name: test
    spec:
      anyKey: any-value
  cr2:
    apiVersion: agent_control.version/v0beta1
    kind: Foo2
    metadata:
      name: test
    # no additional fields
  cr3:
    apiVersion: agent_control.version/v0beta1
    kind: Foo
    metadata:
      name: test
    key: value # no spec field
  cr4:
    apiVersion: agent_control.version/v0beta1
    kind: Foo
    metadata:
      name: test
      labels:
        foo: bar
    key: value # no spec field
"#;

    #[test]
    fn test_k8s_object() {
        let k8s: K8s = serde_yaml::from_str(RUNTIME_WITH_K8S_DEPLOYMENT).unwrap();
        assert_eq!("Foo".to_string(), k8s.objects["cr1"].kind);
        assert_eq!(
            "agent_control.version/v0beta1".to_string(),
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

    #[test]
    fn test_template_k8s() {
        let untouched_val = "${nr-var:any} no templated";
        let test_agent_id = "id";
        let k8s_template: K8s = serde_yaml::from_str(
            format!(
                r#"
objects:
  cr1:
    apiVersion: {untouched_val}
    kind: {untouched_val}
    metadata:
      name: ${{nr-sub:agent_id}}
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
                VariableDefinition::new(String::default(), true, None, Some(value.to_string())),
            ),
            (
                "nr-sub:agent_id".to_string(),
                VariableDefinition::new_final_string_variable(test_agent_id.to_string()),
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

        // Expect template works on labels.
        let labels = cr1.metadata.labels;
        assert_eq!(labels.get("foo").unwrap(), value);
        assert_eq!(labels.get(value).unwrap(), "bar");
    }
}
