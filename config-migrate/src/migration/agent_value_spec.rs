use crate::migration::agent_value_spec::YAMLConfigSpec::{
    YAMLConfigSpecEnd, YAMLConfigSpecMapping,
};
use crate::migration::config::AgentTypeFieldFQN;
use crate::migration::config::{FILE_SEPARATOR, FILE_SEPARATOR_REPLACE};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub enum AgentValueError {
    #[error("error merging YAMLConfigSpecs: {0}")]
    MergeError(String),
}

#[derive(Debug, Deserialize, PartialEq, Serialize, Clone)]
#[serde(untagged)]
pub enum YAMLConfigSpec {
    YAMLConfigSpecEnd(EndSpec),
    YAMLConfigSpecMapping(HashMap<String, YAMLConfigSpec>),
}

impl YAMLConfigSpec {
    #[cfg(test)]
    fn get(&self, key: String) -> Option<YAMLConfigSpec> {
        match self {
            YAMLConfigSpecEnd(_) => None,
            YAMLConfigSpecMapping(m) => Some(m.get(key.clone().as_str()).unwrap().clone()),
        }
    }
}

type EndSpec = String;

pub fn from_fqn_and_value(
    fqn: AgentTypeFieldFQN,
    value: YAMLConfigSpec,
) -> HashMap<String, YAMLConfigSpec> {
    let cloned_fqn = fqn.clone().as_string();
    let mut parts: Vec<&str> = cloned_fqn.rsplit(FILE_SEPARATOR).collect();
    let first = parts.last().unwrap().to_string();
    parts.remove(parts.len() - 1);
    let mut last_node = value;
    for part in parts {
        // restore file separator
        let restored_part = part.replace(FILE_SEPARATOR_REPLACE, FILE_SEPARATOR);
        last_node = YAMLConfigSpecMapping(HashMap::from([(restored_part.to_string(), last_node)]));
    }
    HashMap::from([(first, last_node)])
}

pub fn merge_agent_values(
    agents_values_specs: Vec<HashMap<String, YAMLConfigSpec>>,
) -> Result<HashMap<String, YAMLConfigSpec>, AgentValueError> {
    let mut result: HashMap<String, YAMLConfigSpec> = HashMap::new();
    for agent_values_spec in agents_values_specs {
        merge_agent_values_recursive(agent_values_spec, &mut result);
    }
    Ok(result)
}

/// merge_agent_values_recursive merges tw hashmaps of YAMLConfigSpecs in one respecting the hierarchy
fn merge_agent_values_recursive(
    from: HashMap<String, YAMLConfigSpec>,
    to: &mut HashMap<String, YAMLConfigSpec>,
) {
    for (key, spec) in from {
        match spec.clone() {
            YAMLConfigSpecEnd(_) => {
                to.entry(key).or_insert_with(|| spec);
            }
            YAMLConfigSpecMapping(m) => {
                if to.contains_key(key.as_str()) {
                    let child = &mut to.get(key.clone().as_str()).unwrap();
                    match child.clone() {
                        YAMLConfigSpecEnd(_) => {
                            error!("cannot insert into end_spec")
                        }
                        YAMLConfigSpecMapping(mut m_child) => {
                            merge_agent_values_recursive(m, &mut m_child);
                            to.insert(key, YAMLConfigSpecMapping(m_child));
                        }
                    }
                } else {
                    to.insert(key, spec.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::migration::agent_value_spec::YAMLConfigSpec::{
        YAMLConfigSpecEnd, YAMLConfigSpecMapping,
    };
    use crate::migration::agent_value_spec::{
        from_fqn_and_value, merge_agent_values_recursive, YAMLConfigSpec,
    };
    use crate::migration::config::AgentTypeFieldFQN;
    use std::collections::HashMap;

    #[test]
    fn test_from_fqn_and_value() {
        let fqn: AgentTypeFieldFQN = "one.two.three".into();
        let spec = YAMLConfigSpecEnd("the value".to_string());
        let agent_value = from_fqn_and_value(fqn, spec.clone());
        let val = agent_value
            .get("one")
            .unwrap()
            .get("two".to_string())
            .unwrap()
            .get("three".to_string())
            .unwrap();
        assert_eq!(val, spec);
    }

    // TODO: We need to transform this into tests instead of just printing the results
    #[test]
    fn test_merge_agent_values_recursive() {
        let from: HashMap<String, YAMLConfigSpec> = HashMap::from([
            ("1".to_string(), YAMLConfigSpecEnd("value".to_string())),
            (
                "2".to_string(),
                YAMLConfigSpecMapping(HashMap::from([(
                    "2.1".to_string(),
                    YAMLConfigSpecEnd("value 2.1".to_string()),
                )])),
            ),
        ]);

        let mut to: HashMap<String, YAMLConfigSpec> = HashMap::new();

        merge_agent_values_recursive(from, &mut to);

        println!("{:?}", to);
    }

    #[test]
    fn test_merge_agent_values_recursive_2() {
        let from: HashMap<String, YAMLConfigSpec> = HashMap::from([
            ("1".to_string(), YAMLConfigSpecEnd("value 1".to_string())),
            (
                "2".to_string(),
                YAMLConfigSpecMapping(HashMap::from([(
                    "2.1".to_string(),
                    YAMLConfigSpecEnd("value 2.1".to_string()),
                )])),
            ),
        ]);

        let mut to: HashMap<String, YAMLConfigSpec> = HashMap::from([
            ("3".to_string(), YAMLConfigSpecEnd("value 3".to_string())),
            (
                "4".to_string(),
                YAMLConfigSpecMapping(HashMap::from([(
                    "4.1".to_string(),
                    YAMLConfigSpecEnd("value 4.1".to_string()),
                )])),
            ),
            (
                "2".to_string(),
                YAMLConfigSpecMapping(HashMap::from([(
                    "2.2".to_string(),
                    YAMLConfigSpecEnd("value 2.2".to_string()),
                )])),
            ),
        ]);

        merge_agent_values_recursive(from, &mut to);

        println!("{:?}", to);
    }

    #[test]
    fn test_merge_agent_values_recursive_3() {
        let from: HashMap<String, YAMLConfigSpec> = HashMap::from([(
            "2".to_string(),
            YAMLConfigSpecMapping(HashMap::from([(
                "2.1".to_string(),
                YAMLConfigSpecEnd("value 2.1".to_string()),
            )])),
        )]);

        let mut to: HashMap<String, YAMLConfigSpec> = HashMap::from([(
            "2".to_string(),
            YAMLConfigSpecMapping(HashMap::from([(
                "2.2".to_string(),
                YAMLConfigSpecEnd("value 2.2".to_string()),
            )])),
        )]);

        merge_agent_values_recursive(from, &mut to);

        println!("{:?}", to);
    }
}
