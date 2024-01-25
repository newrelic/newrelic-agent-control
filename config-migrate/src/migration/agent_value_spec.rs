use crate::migration::agent_value_spec::AgentValueSpec::{
    AgentValueSpecEnd, AgentValueSpecMapping,
};
use crate::migration::config::AgentTypeFieldFQN;
use crate::migration::config::{FILE_SEPARATOR, FILE_SEPARATOR_REPLACE};
use log::error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentValueError {
    #[error("error merging AgentValueSpecs: {0}")]
    MergeError(String),
}

#[derive(Debug, Deserialize, PartialEq, Serialize, Clone)]
#[serde(untagged)]
pub enum AgentValueSpec {
    AgentValueSpecEnd(EndSpec),
    AgentValueSpecMapping(HashMap<String, AgentValueSpec>),
}

impl AgentValueSpec {
    #[cfg(test)]
    fn get(&self, key: String) -> Option<AgentValueSpec> {
        match self {
            AgentValueSpecEnd(_) => None,
            AgentValueSpecMapping(m) => Some(m.get(key.clone().as_str()).unwrap().clone()),
        }
    }
}

type EndSpec = String;

pub fn from_fqn_and_value(
    fqn: AgentTypeFieldFQN,
    value: AgentValueSpec,
) -> HashMap<String, AgentValueSpec> {
    let cloned_fqn = fqn.clone().as_string();
    let mut parts: Vec<&str> = cloned_fqn.rsplit(FILE_SEPARATOR).collect();
    let first = parts.last().unwrap().to_string();
    parts.remove(parts.len() - 1);
    let mut last_node = value;
    for part in parts {
        // restore file separator
        let restored_part = part.replace(FILE_SEPARATOR_REPLACE, FILE_SEPARATOR);
        last_node = AgentValueSpecMapping(HashMap::from([(restored_part.to_string(), last_node)]));
    }
    HashMap::from([(first, last_node)])
}

pub fn merge_agent_values(
    agents_values_specs: Vec<HashMap<String, AgentValueSpec>>,
) -> Result<HashMap<String, AgentValueSpec>, AgentValueError> {
    let mut result: HashMap<String, AgentValueSpec> = HashMap::new();
    for agent_values_spec in agents_values_specs {
        merge_agent_values_recursive(agent_values_spec, &mut result);
    }
    Ok(result)
}

/// merge_agent_values_recursive merges tw hashmaps of AgentValueSpecs in one respecting the hierarchy
fn merge_agent_values_recursive(
    from: HashMap<String, AgentValueSpec>,
    to: &mut HashMap<String, AgentValueSpec>,
) {
    for (key, spec) in from {
        match spec.clone() {
            AgentValueSpecEnd(_) => {
                to.entry(key).or_insert_with(|| spec);
            }
            AgentValueSpecMapping(m) => {
                if to.contains_key(key.as_str()) {
                    let child = &mut to.get(key.clone().as_str()).unwrap();
                    match child.clone() {
                        AgentValueSpecEnd(_) => {
                            error!("cannot insert into end_spec")
                        }
                        AgentValueSpecMapping(mut m_child) => {
                            merge_agent_values_recursive(m, &mut m_child);
                            to.insert(key, AgentValueSpecMapping(m_child));
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
    use crate::migration::agent_value_spec::AgentValueSpec::{
        AgentValueSpecEnd, AgentValueSpecMapping,
    };
    use crate::migration::agent_value_spec::{
        from_fqn_and_value, merge_agent_values_recursive, AgentValueSpec,
    };
    use crate::migration::config::AgentTypeFieldFQN;
    use std::collections::HashMap;

    #[test]
    fn test_from_fqn_and_value() {
        let fqn: AgentTypeFieldFQN = "one.two.three".into();
        let spec = AgentValueSpecEnd("the value".to_string());
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
        let from: HashMap<String, AgentValueSpec> = HashMap::from([
            ("1".to_string(), AgentValueSpecEnd("value".to_string())),
            (
                "2".to_string(),
                AgentValueSpecMapping(HashMap::from([(
                    "2.1".to_string(),
                    AgentValueSpecEnd("value 2.1".to_string()),
                )])),
            ),
        ]);

        let mut to: HashMap<String, AgentValueSpec> = HashMap::new();

        merge_agent_values_recursive(from, &mut to);

        println!("{:?}", to);
    }

    #[test]
    fn test_merge_agent_values_recursive_2() {
        let from: HashMap<String, AgentValueSpec> = HashMap::from([
            ("1".to_string(), AgentValueSpecEnd("value 1".to_string())),
            (
                "2".to_string(),
                AgentValueSpecMapping(HashMap::from([(
                    "2.1".to_string(),
                    AgentValueSpecEnd("value 2.1".to_string()),
                )])),
            ),
        ]);

        let mut to: HashMap<String, AgentValueSpec> = HashMap::from([
            ("3".to_string(), AgentValueSpecEnd("value 3".to_string())),
            (
                "4".to_string(),
                AgentValueSpecMapping(HashMap::from([(
                    "4.1".to_string(),
                    AgentValueSpecEnd("value 4.1".to_string()),
                )])),
            ),
            (
                "2".to_string(),
                AgentValueSpecMapping(HashMap::from([(
                    "2.2".to_string(),
                    AgentValueSpecEnd("value 2.2".to_string()),
                )])),
            ),
        ]);

        merge_agent_values_recursive(from, &mut to);

        println!("{:?}", to);
    }

    #[test]
    fn test_merge_agent_values_recursive_3() {
        let from: HashMap<String, AgentValueSpec> = HashMap::from([(
            "2".to_string(),
            AgentValueSpecMapping(HashMap::from([(
                "2.1".to_string(),
                AgentValueSpecEnd("value 2.1".to_string()),
            )])),
        )]);

        let mut to: HashMap<String, AgentValueSpec> = HashMap::from([(
            "2".to_string(),
            AgentValueSpecMapping(HashMap::from([(
                "2.2".to_string(),
                AgentValueSpecEnd("value 2.2".to_string()),
            )])),
        )]);

        merge_agent_values_recursive(from, &mut to);

        println!("{:?}", to);
    }
}
