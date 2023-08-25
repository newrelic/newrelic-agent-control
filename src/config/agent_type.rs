use regex::Regex;
use serde::Deserialize;
use serde_yaml::Value;
use std::{collections::HashMap as Map, io};
use thiserror::Error;

use crate::config::supervisor_config::{FilePathWithContent, N};

use super::supervisor_config::{
    normalize_supervisor_config, validate_with_agent_type, NormalizedSupervisorConfig,
    SupervisorConfig, TrivialValue,
};

const TEMPLATE_RE: &str = r"\$\{([a-zA-Z0-9\.\-_/]+)\}";
const TEMPLATE_BEGIN: &str = "${";
const TEMPLATE_END: char = '}';
pub(crate) const TEMPLATE_KEY_SEPARATOR: &str = ".";

#[derive(Error, Debug)]
pub(crate) enum AgentTypeError {
    #[error("`{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error("Missing required key in config: `{0}`")]
    MissingAgentKey(String),
    #[error("Type mismatch for key `{key}` in config: expected a {expected_type:?}, got {actual_value:?}")]
    MismatchedTypes {
        key: String,
        expected_type: SpecType,
        actual_value: TrivialValue,
    },
    #[error("Found unexpected keys in config: {0:?}")]
    UnexpectedKeysInConfig(Vec<String>),
    #[error("I/O error: `{0}`")]
    IOError(#[from] io::Error),
    #[error("Attempted to store an invalid path on a FilePathWithContent object")]
    InvalidFilePath,
    #[error("Missing required template key: `{0}`")]
    MissingTemplateKey(String),

    #[error("Missing default value for spec key `{0}`")]
    MissingDefaultForSpecKey(String),
    #[error("Invalid default value for spec key `{key}`: expected a {type_:?}")]
    InvalidDefaultForSpec { key: String, type_: SpecType },
}

#[derive(Debug, Deserialize)]
struct RawAgent {
    name: String,
    namespace: String,
    version: String,
    spec: AgentSpec,
    #[serde(default)]
    meta: Meta,
}

#[derive(Debug, PartialEq, Clone, Default)]
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

    fn populate(self, config: SupervisorConfig) -> Result<Self, AgentTypeError> {
        let normalized_config = normalize_supervisor_config(config);
        let validated_conf = validate_with_agent_type(normalized_config, &self)?;

        let meta = self.meta.template_with(validated_conf.clone())?;
        let mut spec = self.spec;

        validated_conf.into_iter().for_each(|(k, v)| {
            spec.entry(k).and_modify(|s| {
                s.final_value = Some(v);
            });
        });

        Ok(AgentType { meta, spec, ..self })
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
    pub(crate) final_value: Option<TrivialValue>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub(crate) enum SpecType {
    #[serde(rename = "string")]
    String,
    #[serde(rename = "bool")]
    Bool,
    #[serde(rename = "number")]
    Number,
    #[serde(rename = "file")]
    File,
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
            "file" => ST::File,
            // "map[string]string" => ST::MapStringString,
            // "map[string]number" => ST::MapStringNumber,
            // "map[string]bool" => ST::MapStringBool,
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
            Some(Value::String(s)) => Some(if type_ == ST::File {
                TrivialValue::File(FilePathWithContent::new(s))
            } else {
                TrivialValue::String(s)
            }),
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
            final_value: None,
        })
    }
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
struct Meta {
    deployment: Deployment,
}

impl Templateable for Meta {
    fn template_with(self, kv: NormalizedSupervisorConfig) -> Result<Self, AgentTypeError> {
        Ok(Meta {
            deployment: self.deployment.template_with(kv)?,
        })
    }
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
struct Deployment {
    on_host: Option<OnHost>,
}

impl Templateable for Deployment {
    fn template_with(self, kv: NormalizedSupervisorConfig) -> Result<Self, AgentTypeError> {
        /*
        `self.on_host` has type `Option<OnHost>`

        let t = self.on_host.map(|o| o.template_with(kv)); `t` has type `Option<Result<OnHost, AgentTypeError>>`

        Let's visit all the possibilities of `t`.
        When I do `t.transpose()`, which takes an Option<Result<_,_>> and returns a Result<Option<_>,_>, this is what happens:

        ```
        match t {
            None => Ok(None),
            Some(Ok(on_host)) => Ok(Some(on_host)),
            Some(Err(e)) => Err(e),
        }
        ```

        In general:
        - None will be mapped to Ok(None).
        - Some(Ok(_)) will be mapped to Ok(Some(_)).
        - Some(Err(_)) will be mapped to Err(_).

        With `?` I get rid of the original Result<_,_> wrapper type and get the Option<_> (or else the error bubbles up if it contained the Err(_) variant). Then I am able to store that Option<_>, be it None or Some(_), back into the Deployment object which contains the Option<_> field.
         */

        let oh = self.on_host.map(|oh| oh.template_with(kv)).transpose()?;
        Ok(Deployment { on_host: oh })
    }
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
struct OnHost {
    executables: Vec<Executable>,
}

impl Templateable for OnHost {
    fn template_with(self, kv: NormalizedSupervisorConfig) -> Result<Self, AgentTypeError> {
        Ok(OnHost {
            executables: self
                .executables
                .into_iter()
                .map(|e| e.template_with(kv.clone()))
                .collect::<Result<Vec<Executable>, AgentTypeError>>()?,
        })
    }
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
struct Executable {
    path: String,
    args: Args,
    env: Env,
}

trait IntoVector<T> {
    fn into_vector(self) -> Vec<T>;
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
struct Args(String);

impl IntoVector<String> for Args {
    fn into_vector(self) -> Vec<String> {
        self.0.split_whitespace().map(|s| s.to_string()).collect()
    }
}

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
struct Env(String);

impl IntoVector<(String, String)> for Env {
    fn into_vector(self) -> Vec<(String, String)> {
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

trait Templateable {
    fn template_with(self, kv: Map<String, TrivialValue>) -> Result<Self, AgentTypeError>
    where
        Self: std::marker::Sized;
}

impl Templateable for Executable {
    fn template_with(self, kv: Map<String, TrivialValue>) -> Result<Executable, AgentTypeError> {
        let re = Regex::new(TEMPLATE_RE).unwrap();
        let mut result = self;

        let path = result.path.clone();
        let res = re
            .find_iter(&path)
            .map(|i| i.as_str())
            .collect::<Vec<&str>>();

        for i in res {
            let trimmed_s = i
                .trim_start_matches(TEMPLATE_BEGIN)
                .trim_end_matches(TEMPLATE_END);
            if !kv.contains_key(trimmed_s) {
                return Err(AgentTypeError::MissingTemplateKey(trimmed_s.to_string()));
            }
            // if let TrivialValue::String(replacement) = &kv[trimmed_s] {
            let replacement = &kv[trimmed_s];
            result.path = re
                .replace(&result.path, replacement.to_string())
                .to_string();
        }

        // Same for args
        let args = result.args.clone();
        let res = re
            .find_iter(&args.0)
            .map(|i| i.as_str())
            .collect::<Vec<&str>>();

        for i in res {
            let trimmed_s = i.trim_start_matches("${").trim_end_matches('}');
            if !kv.contains_key(trimmed_s) {
                return Err(AgentTypeError::MissingTemplateKey(trimmed_s.to_string()));
            }
            let replacement = &kv[trimmed_s];
            result.args = Args(
                re.replace(&result.args.0, replacement.to_string())
                    .to_string(),
            );
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

fn normalize_agent_spec(spec: AgentSpec) -> Result<NormalizedSpec, AgentTypeError> {
    use SpecType as ST;

    let mut result = Map::new();

    for (k, v) in spec {
        let n_spec = inner_normalize(k, v);
        for (k, v) in n_spec.iter() {
            if v.default.is_none() && !v.required {
                return Err(AgentTypeError::MissingDefaultForSpecKey(k.clone()));
            }
            if let Some(default) = v.default.as_ref() {
                match default {
                    TrivialValue::String(_) if v.type_ == ST::String => {}
                    TrivialValue::Bool(_) if v.type_ == ST::Bool => {}
                    TrivialValue::Number(_) if v.type_ == ST::Number => {}
                    TrivialValue::File(_) if v.type_ == ST::File => {}
                    // TrivialValue::Mapping(_)
                    //     if (v.type_ == ST::MapStringString
                    //         || v.type_ == ST::MapStringBool
                    //         || v.type_ == ST::MapStringNumber) => {}
                    _ => {
                        return Err(AgentTypeError::InvalidDefaultForSpec {
                            key: k.clone(),
                            type_: v.type_.clone(),
                        })
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
        Spec::SpecMapping(m) => m.into_iter().for_each(|(k, v)| {
            result.extend(inner_normalize(
                key.clone() + TEMPLATE_KEY_SEPARATOR + &k,
                v,
            ))
        }),
    }
    result
}

#[cfg(test)]
mod tests {
    use crate::config::supervisor_config::SupervisorConfig;

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
          env: ""
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
          env: ""
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
            Args("-c ${deployment.k8s.image}".to_string()),
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
                final_value: None,
            },
        )]);

        // expect output to be the map

        assert_eq!(expected_map, given_agent.spec);

        let expected_spec = EndSpec {
            description: "Name of the agent".to_string(),
            type_: SpecType::String,
            required: false,
            default: Some(TrivialValue::String("nrdot".to_string())),
            final_value: None,
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
            args: Args(
                "--verbose ${deployment.on_host.verbose} --logs ${deployment.on_host.log_level}"
                    .to_string(),
            ),
            env: Env("".to_string()),
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
            args: Args("--verbose true --logs trace".to_string()),
            env: Env("".to_string()),
        };

        assert_eq!(exec_actual, exec_expected);
    }

    #[test]
    fn test_replacer_two_same() {
        let exec = Executable {
            path: "${bin}/otelcol".to_string(),
            args: Args("--verbose ${deployment.on_host.verbose} --verbose_again ${deployment.on_host.verbose}"
                .to_string()),
            env: Env("".to_string()),
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
            args: Args("--verbose true --verbose_again true".to_string()),
            env: Env("".to_string()),
        };

        assert_eq!(exec_actual, exec_expected);
    }

    const GIVEN_NEWRELIC_INFRA_YAML: &str = r#"
name: newrelic-infra
namespace: newrelic
version: 1.39.1
spec:
  config:
    description: "Newrelic infra configuration yaml"
    type: file
    required: true
meta:
  deployment:
    on_host:
      executables:
        - path: /usr/bin/newrelic-infra
          args: "--config ${config}"
          env: ""
"#;

    const GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML: &str = r#"
config: | 
    license: abc123
    staging: true
"#;

    #[test]
    fn test_populate_meta_field() {
        let input_agent_type =
            serde_yaml::from_str::<AgentType>(GIVEN_NEWRELIC_INFRA_YAML).unwrap();
        let input_user_config =
            serde_yaml::from_str::<SupervisorConfig>(GIVEN_NEWRELIC_INFRA_USER_CONFIG_YAML)
                .unwrap();

        let actual = input_agent_type
            .populate(input_user_config)
            .expect("Failed to populate the AgentType's Meta field");

        println!("Output: {:#?}", actual);
    }
}
