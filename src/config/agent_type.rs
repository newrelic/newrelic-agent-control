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

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct AgentType {
    name: String,
    namespace: String,
    version: String,
    pub(crate) spec: NormalizedSpec,
    meta: Meta,
}

impl AgentType {
    fn get_spec(self, path: String) -> Option<EndSpec> {
        self.spec.get(&path).cloned()
    }
}

impl From<RawAgent> for AgentType {
    fn from(raw_agent: RawAgent) -> Self {
        let normalized_agent = normalize_agent_spec(raw_agent.spec);
        AgentType {
            spec: normalized_agent,
            name: raw_agent.name,
            namespace: raw_agent.namespace,
            version: raw_agent.version,
            meta: raw_agent.meta,
        }
    }
}

impl<'de> Deserialize<'de> for AgentType {
    fn deserialize<D>(deserializer: D) -> Result<AgentType, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw_agent: RawAgent = RawAgent::deserialize(deserializer)?;
        Ok(AgentType::from(raw_agent))
    }
}

type AgentSpec = HashMap<String, Spec>;

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct EndSpec {
    description: String,
    pub(crate) type_: SpecType,
    pub(crate) required: bool,
    pub(crate) default: Value,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub(crate) enum SpecType {
    #[serde(rename = "string")]
    String,
    #[serde(rename = "bool")]
    Bool,
    #[serde(rename = "number")]
    Number,
    #[serde(rename = "map[string]string")]
    MapStringString,
    #[serde(rename = "map[string]number")]
    MapStringNumber,
    #[serde(rename = "map[string]bool")]
    MapStringBool,
}

impl<'de> Deserialize<'de> for EndSpec {
    fn deserialize<D>(deserializer: D) -> Result<EndSpec, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as E;
        use serde_yaml::Value as V;
        use SpecType as ST;

        let mut map: HashMap<String, Value> = HashMap::deserialize(deserializer)?;
        let description = map
            .remove("description")
            .ok_or(E::custom("Could not get `description` field"))?
            .as_str()
            .ok_or(E::custom("`description` field is not a string"))?
            .to_string();

        let type_b = map
            .remove("type")
            .ok_or(E::custom("Could not get `type` field"))?;
        let type_str = type_b
            .as_str()
            .ok_or(E::custom("`type` field is not a string"))?;
        let type_ = match type_str {
            "string" => ST::String,
            "boolean" => ST::Bool,
            "number" => ST::Number,
            "map[string]string" => ST::MapStringString,
            "map[string]number" => ST::MapStringNumber,
            "map[string]bool" => ST::MapStringBool,
            x => return Err(E::custom(format!("Invalid type: {}", x))),
        };

        let required = map
            .remove("required")
            .ok_or(E::custom("Could not get `required` field"))?
            .as_bool()
            .ok_or(E::custom("`required` field is not a boolean"))?;
        let default = map
            .remove("default")
            .ok_or(E::custom("Could not get `default` field"))?;

        match default.clone() {
            V::String(_) if type_ == ST::String => {}
            V::Bool(_) if type_ == ST::Bool => {}
            V::Number(_) if type_ == ST::Number => {}
            V::Mapping(_)
                if (type_ == ST::MapStringString
                    || type_ == ST::MapStringBool
                    || type_ == ST::MapStringNumber) => {}

            _ => {
                return Err(E::custom(
                    "Invalid default value (invalid data or data does not match `type`)",
                ))
            }
        }
        Ok(EndSpec {
            description,
            type_,
            required,
            default,
        })
    }
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
struct Meta {
    deployment: Deployment,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
struct Deployment {
    on_host: Option<OnHost>,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
struct OnHost {
    executables: Vec<Executable>,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
struct Executable {
    path: String,
    args: String,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
struct K8s {
    crd: String,
}

// Spec can be an arbitrary number of nested mappings but all node terminal leaves are EndSpec,
// so a recursive datatype is the answer!
#[derive(Debug, Deserialize, PartialEq)]
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
    let mut result = HashMap::new();
    match spec {
        Spec::SpecEnd(s) => _ = result.insert(key, s),
        Spec::SpecMapping(m) => m
            .into_iter()
            .for_each(|(k, v)| result.extend(inner_normalize(key.clone() + "/" + &k, v))),
    }
    result
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

        let agent = AgentType::from(raw_agent);

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
            "spec: data did not match any variant of untagged enum Spec at line 6 column 3"
        );
    }

    #[test]
    fn test_normalize_agent_spec() {
        // create AgentSpec

        let given_agent_config: RawAgent = serde_yaml::from_str(GIVEN_YAML).unwrap();

        // println!("agent: {:#?}", given_agent_config);

        let given_agent = AgentType::from(given_agent_config);

        let expected_map: HashMap<String, EndSpec> = HashMap::from([(
            "description/name".to_string(),
            EndSpec {
                description: "Name of the agent".to_string(),
                type_: SpecType::String,
                required: false,
                default: Value::String("nrdot".to_string()),
            },
        )]);

        // expect output to be the map

        assert_eq!(expected_map, given_agent.spec);

        let expected_spec = EndSpec {
            description: "Name of the agent".to_string(),
            type_: SpecType::String,
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
