use serde::Deserialize;
use serde_yaml::Value;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct RawAgent {
    name: String,
    namespace: String,
    version: String,
    spec: AgentSpec,
    #[serde(default)]
    meta: Meta,
}

struct Agent {
    name: String,
    namespace: String,
    version: String,
    spec: NormalizedSpec,
    meta: Meta,
}

impl Agent {
    fn get_spec(self, path: String) -> Option<EndSpec> {
        self.spec.get(&path).cloned()
    }
}

impl From<RawAgent> for Agent {
    fn from(raw_agent: RawAgent) -> Self {
        let normalized_agent = normalize_agent_spec(raw_agent.spec);
        Agent {
            spec: normalized_agent,
            name: raw_agent.name,
            namespace: raw_agent.namespace,
            version: raw_agent.version,
            meta: raw_agent.meta,
        }
    }
}

type AgentSpec = HashMap<String, Spec>;

#[derive(Debug, Deserialize, PartialEq, Clone)]
struct EndSpec {
    description: String,
    #[serde(rename = "type")]
    type_: String,
    required: bool,
    default: Value,
}

#[derive(Debug, Deserialize, Default)]
struct Meta {
    deployment: Deployment,
}

#[derive(Debug, Deserialize, Default)]
struct Deployment {
    on_host: Option<OnHost>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct OnHost {
    executables: Vec<Executable>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct Executable {
    path: String,
    args: String,
}

#[derive(Debug, Deserialize, Default)]
struct K8s {
    crd: String,
}

// Spec can be an arbitrary number of nested mappings but all node terminal leaves are EndSpec,
// so a recursive datatype is the answer!
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Spec {
    SpecEnd(EndSpec),
    SpecMapping(HashMap<String, Spec>),
}

type NormalizedSpec = HashMap<String, EndSpec>;

fn normalize_agent_spec(spec: AgentSpec) -> NormalizedSpec {
    let mut result = HashMap::new();
    spec.into_iter()
        .for_each(|(k, v)| result.extend(inner_normalize(k, v)));
    result
}

fn inner_normalize(key: String, spec: Spec) -> NormalizedSpec {
    let mut mapping = HashMap::new();
    match spec {
        Spec::SpecEnd(s) => _ = mapping.insert(key, s),
        Spec::SpecMapping(m) => m
            .into_iter()
            .for_each(|(k, v)| mapping.extend(inner_normalize(key.clone() + "/" + &k, v))),
    }
    mapping
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::{Error, Value};
    use std::collections::HashMap;

    const GIVEN_YAML: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
spec:
  description:
    name:
      description: "Name of the agent"
      type: string
      required: false
      default: nrdot
meta:
  deployment:
    on_host:
      executables:
        - path: ${bin}/otelcol
          args: "-c ${deployment.k8s.image}"
"#;

    const GIVEN_BAD_YAML: &str = r#"
name: nrdot
namespace: newrelic
version: 0.1.0
spec:
  description:
    name:
meta:
  deployment:
    on_host:
      executables:
        - path: ${bin}/otelcol
          args: "-c ${deployment.k8s.image}"
"#;

    #[test]
    fn test_basic_parsing() {
        let raw_agent: RawAgent = serde_yaml::from_str(GIVEN_YAML).unwrap();

        let agent = Agent::from(raw_agent);

        assert_eq!("nrdot", agent.name);
        assert_eq!("newrelic", agent.namespace);
        assert_eq!("0.1.0", agent.version);

        assert_eq!(
            "${bin}/otelcol",
            agent.meta.deployment.on_host.clone().unwrap().executables[0].path
        );
        assert_eq!(
            "-c ${deployment.k8s.image}",
            agent.meta.deployment.on_host.unwrap().executables[0].args
        );
    }

    #[test]
    fn test_bad_parsing() {
        let raw_agent_err: Result<RawAgent, Error> = serde_yaml::from_str(GIVEN_BAD_YAML);

        assert!(raw_agent_err.is_err());
        assert_eq!(
            raw_agent_err.unwrap_err().to_string(),
            "spec: data did not match any variant of untagged enum Spec at line 7 column 3"
        );
    }

    #[test]
    fn test_normalize_agent_spec() {
        // create AgentSpec

        let given_agent_config: RawAgent = serde_yaml::from_str(GIVEN_YAML).unwrap();

        println!("agent: {:#?}", given_agent_config);

        let given_agent = Agent::from(given_agent_config);

        let expected_map: HashMap<String, EndSpec> = HashMap::from([(
            "description/name".to_string(),
            EndSpec {
                description: "Name of the agent".to_string(),
                type_: "string".to_string(),
                required: false,
                default: Value::String("nrdot".to_string()),
            },
        )]);

        // expect output to be the map

        assert_eq!(expected_map, given_agent.spec);

        let expected_spec = EndSpec {
            description: "Name of the agent".to_string(),
            type_: "string".to_string(),
            required: false,
            default: Value::String("nrdot".to_string()),
        };

        assert_eq!(
            expected_spec,
            given_agent
                .get_spec("description/name".to_string())
                .unwrap()
        );
    }
}
