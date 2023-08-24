use regex::Regex;
use serde::Deserialize;
use serde_yaml::{Number, Value};
use std::collections::HashMap as Map;

use crate::config::supervisor_config::N;

use super::supervisor_config::TrivialValue;

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

impl<'de> Deserialize<'de> for AgentType {
    fn deserialize<D>(deserializer: D) -> Result<AgentType, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw_agent: RawAgent = RawAgent::deserialize(deserializer)?;
        let normalized_agent =
            normalize_agent_spec(raw_agent.spec).map_err(serde::de::Error::custom)?;
        Ok(AgentType {
            spec: normalized_agent,
            name: raw_agent.name,
            namespace: raw_agent.namespace,
            version: raw_agent.version,
            meta: raw_agent.meta,
        })
    }
}

type AgentSpec = Map<String, Spec>;

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct EndSpec {
    description: String,
    pub(crate) type_: SpecType,
    pub(crate) required: bool,
    pub(crate) default: Option<TrivialValue>,
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
        use SpecType as ST;

        let mut map: Map<String, Value> = Map::deserialize(deserializer)?;
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
        let default = map.remove("default");

        if default.is_none() && !required {
            return Err(E::custom(
                "Missing `default` field for a non-required value.",
            ));
        }

        let default = match default {
            Some(Value::Bool(b)) => Some(TrivialValue::Bool(b)),
            Some(Value::String(s)) => Some(TrivialValue::String(s)),
            Some(Value::Number(n)) if n.is_u64() => {
                Some(TrivialValue::Number(N::PosInt(n.as_u64().unwrap())))
            }
            Some(Value::Number(n)) if n.is_i64() => {
                Some(TrivialValue::Number(N::NegInt(n.as_i64().unwrap())))
            }
            Some(Value::Number(n)) if n.is_f64() => {
                Some(TrivialValue::Number(N::Float(n.as_f64().unwrap())))
            }
            Some(_) => None,
            None => None,
        };

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

trait Templateable {
    fn template_with(self, kv: Map<String, TrivialValue>) -> Result<Self, String>
    where
        Self: std::marker::Sized;
}

impl Templateable for Executable {
    fn template_with(self, kv: Map<String, TrivialValue>) -> Result<Executable, String> {
        const RE: &str = r"\$\{([a-zA-Z0-9\.\-_/]+)\}";
        let re = Regex::new(RE).unwrap();
        let mut result = Executable {
            path: self.path,
            args: self.args,
        };

        let path = result.path.clone();
        let res = re
            .find_iter(&path)
            .map(|i| i.as_str())
            .collect::<Vec<&str>>();

        for i in res {
            let trimmed_s = i.trim_start_matches("${").trim_end_matches('}');
            if !kv.contains_key(trimmed_s) {
                return Err(format!("Missing required template key: {trimmed_s}"));
            }
            if let TrivialValue::String(replacement) = &kv[trimmed_s] {
                result.path = re.replace(&result.path, replacement.as_str()).to_string();
            } else {
                return Err(format!(
                    "Invalid value to replace in template for key {trimmed_s}"
                ));
            }
        }

        // Same for args
        let args = result.args.clone();
        let res = re
            .find_iter(&args)
            .map(|i| i.as_str())
            .collect::<Vec<&str>>();

        for i in res {
            let trimmed_s = i.trim_start_matches("${").trim_end_matches('}');
            if !kv.contains_key(trimmed_s) {
                return Err(format!("Missing required template key: {trimmed_s}"));
            }
            if let TrivialValue::String(replacement) = &kv[trimmed_s] {
                result.args = re.replace(&result.args, replacement.as_str()).to_string();
            } else {
                return Err(format!(
                    "Invalid value to replace in template for key {trimmed_s}"
                ));
            }
        }

        Ok(result)
    }
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
    SpecMapping(Map<String, Spec>),
}

type NormalizedSpec = Map<String, EndSpec>;

fn normalize_agent_spec(spec: AgentSpec) -> Result<NormalizedSpec, String> {
    use SpecType as ST;

    let mut result = Map::new();

    for (k, v) in spec {
        let n_spec = inner_normalize(k, v);
        for (k, v) in n_spec.iter() {
            if v.default.is_none() && !v.required {
                return Err(format!("Missing `default` field for key {k}"));
            }
            if let Some(default) = v.default.as_ref() {
                match default {
                    TrivialValue::String(_) if v.type_ == ST::String => {}
                    TrivialValue::Bool(_) if v.type_ == ST::Bool => {}
                    TrivialValue::Number(_) if v.type_ == ST::Number => {}
                    // TrivialValue::Mapping(_)
                    //     if (v.type_ == ST::MapStringString
                    //         || v.type_ == ST::MapStringBool
                    //         || v.type_ == ST::MapStringNumber) => {}
                    _ => {
                        return Err(
                            "Invalid default value (invalid data or data does not match `type`)"
                                .to_string(),
                        )
                    }
                }
            }
        }
        result.extend(n_spec);
    }

    Ok(result)
}

fn inner_normalize(key: String, spec: Spec) -> NormalizedSpec {
    let mut result = Map::new();
    match spec {
        Spec::SpecEnd(s) => _ = result.insert(key, s),
        Spec::SpecMapping(m) => m
            .into_iter()
            .for_each(|(k, v)| result.extend(inner_normalize(key.clone() + "." + &k, v))),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::Error;
    use std::collections::HashMap as Map;

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
        let agent: AgentType = serde_yaml::from_str(GIVEN_YAML).unwrap();

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

        let given_agent: AgentType = serde_yaml::from_str(GIVEN_YAML).unwrap();

        let expected_map: Map<String, EndSpec> = Map::from([(
            "description.name".to_string(),
            EndSpec {
                description: "Name of the agent".to_string(),
                type_: SpecType::String,
                required: false,
                default: Some(TrivialValue::String("nrdot".to_string())),
            },
        )]);

        // expect output to be the map

        assert_eq!(expected_map, given_agent.spec);

        let expected_spec = EndSpec {
            description: "Name of the agent".to_string(),
            type_: SpecType::String,
            required: false,
            default: Some(TrivialValue::String("nrdot".to_string())),
        };

        assert_eq!(
            expected_spec,
            given_agent
                .get_spec("description.name".to_string())
                .unwrap()
        );
    }

    #[test]
    fn test_replacer() {
        let exec = Executable {
            path: "${bin}/otelcol".to_string(),
            args: "--verbose ${deployment.on_host.verbose} --logs ${deployment.on_host.log_level}"
                .to_string(),
        };

        let user_values = Map::from([
            ("bin".to_string(), TrivialValue::String("/etc".to_string())),
            (
                "deployment.on_host.verbose".to_string(),
                TrivialValue::String("true".to_string()),
            ),
            (
                "deployment.on_host.log_level".to_string(),
                TrivialValue::String("trace".to_string()),
            ),
        ]);

        let exec_actual = exec.template_with(user_values).unwrap();

        let exec_expected = Executable {
            path: "/etc/otelcol".to_string(),
            args: "--verbose true --logs trace".to_string(),
        };

        assert_eq!(exec_actual, exec_expected);
    }

    #[test]
    fn test_replacer_two_same() {
        let exec = Executable {
            path: "${bin}/otelcol".to_string(),
            args: "--verbose ${deployment.on_host.verbose} --verbose_again ${deployment.on_host.verbose}"
                .to_string(),
        };

        let user_values = Map::from([
            ("bin".to_string(), TrivialValue::String("/etc".to_string())),
            (
                "deployment.on_host.verbose".to_string(),
                TrivialValue::String("true".to_string()),
            ),
        ]);

        let exec_actual = exec.template_with(user_values).unwrap();

        let exec_expected = Executable {
            path: "/etc/otelcol".to_string(),
            args: "--verbose true --verbose_again true".to_string(),
        };

        assert_eq!(exec_actual, exec_expected);
    }
}
