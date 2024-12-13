use crate::agent_control::config::AgentTypeFQN;
use std::collections::BTreeMap;

const AGENT_FQN_ANNOTATION_KEY: &str = "newrelic.io/agent-type-fqn";

/// Collection of annotations used to identify agent control resources.
#[derive(PartialEq, Default)]
pub struct Annotations(BTreeMap<String, String>);

impl Annotations {
    pub fn new_agent_fqn_annotation(agent_type: &AgentTypeFQN) -> Self {
        let mut annotations = Self::default();
        annotations
            .0
            .insert(AGENT_FQN_ANNOTATION_KEY.to_string(), agent_type.to_string());
        annotations
    }

    pub fn get(&self) -> BTreeMap<String, String> {
        self.0.clone()
    }
}

pub fn get_agent_fqn_value(annotations: &BTreeMap<String, String>) -> Option<&String> {
    annotations.get(AGENT_FQN_ANNOTATION_KEY)
}
