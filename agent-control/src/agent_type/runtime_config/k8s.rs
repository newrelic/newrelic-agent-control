use crate::agent_type::definition::Variables;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::templates::Templateable;
use crate::health::health_checker::{HealthCheckInterval, InitialDelay};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Display;

/// The definition for an K8s supervisor.
///
/// It contains the instructions of what are the agent resources to be managed by the agent-control.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct K8s {
    pub objects: HashMap<String, K8sObject>,
    pub health: Option<K8sHealthConfig>,
}

impl Display for K8s {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let objects_string = self
            .objects
            .iter()
            .map(|(k, v)| {
                let values = v.to_string().replace("\n", "\n    ");
                format!("  {k}:\n    {values}")
            })
            .collect::<Vec<_>>()
            .join("");
        let health_string = self
            .health
            .as_ref()
            .map_or_else(|| "".to_string(), |h| format!(", health: {}", h));
        write!(f, "objects:\n{objects_string}{health_string}")
    }
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

impl Display for K8sObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut fields_string = serde_yaml::to_string(&self.fields).unwrap_or("".to_string());
        if fields_string == "{}\n" {
            fields_string = String::new();
        } else {
            fields_string = fields_string.replace("\n", "\n  ");
        }
        write!(
            f,
            "apiVersion: {}\nkind: {}\n{}\n{fields_string}",
            self.api_version,
            self.kind,
            self.metadata,
        )
    }
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct K8sObjectMeta {
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub name: String,
    pub namespace: String,
}

impl Display for K8sObjectMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let labels_string = self
            .labels
            .iter()
            .map(|(k, v)| {
                let values = v.replace("\n", "\n  ");
                format!("    {k}: {values}" )
            })
            .collect::<Vec<_>>()
            .join("\n");
        if labels_string.is_empty() {
            write!(f, "metadata:\n  name: {}\n  namespace: {}", self.name, self.namespace)
        } else {
            write!(
                f,
                "metadata:\n  name: {}\n  namespace: {}\n  labels:\n{}",
                self.name, self.namespace, labels_string
            )
        }
    }
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
    #[serde(default)]
    pub(crate) interval: HealthCheckInterval,
    /// The initial delay before the first health check is performed.
    #[serde(default)]
    pub(crate) initial_delay: InitialDelay,
}

impl Display for K8sHealthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "interval: {}, initial_delay: {}",
            self.interval, self.initial_delay
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::agent_type::definition::Variables;
    use crate::agent_type::runtime_config::k8s::K8s;
    use crate::agent_type::templates::Templateable;
    use crate::agent_type::variable::Variable;

    const RUNTIME_WITH_K8S_DEPLOYMENT: &str = r#"
objects:
  cr1:
    apiVersion: agent_control.version/v0beta1
    kind: Foo
    metadata:
      name: test
      namespace: test-namespace
      labels:
        foo: bar
        bar: baz
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
        let test_namespace = "test-namespace";
        let k8s_template: K8s = serde_yaml::from_str(
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
}
