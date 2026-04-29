use crate::agent_type::agent_type_id::AgentTypeID;
use std::collections::BTreeMap;

const AGENT_TYPE_ID_ANNOTATION_KEY: &str = "newrelic.io/agent-type-id";
const OWNED_BY_ANNOTATION_KEY: &str = "newrelic.io/owned-by";

const OWNED_BY_AGENT_CONTROL: &str = "agent-control";
const OWNED_BY_SUB_AGENT: &str = "sub-agent";

/// Collection of annotations used to identify agent control resources.
#[derive(PartialEq, Default)]
pub struct Annotations(BTreeMap<String, String>);

impl Annotations {
    pub fn new_agent_type_id_annotation(agent_type: &AgentTypeID) -> Self {
        Self(BTreeMap::from([(
            AGENT_TYPE_ID_ANNOTATION_KEY.to_string(),
            agent_type.to_string(),
        )]))
    }

    pub fn new_agent_control_owned(agent_type_id: Option<&AgentTypeID>) -> Self {
        let mut map = BTreeMap::from([(
            OWNED_BY_ANNOTATION_KEY.to_string(),
            OWNED_BY_AGENT_CONTROL.to_string(),
        )]);
        agent_type_id.inspect(|agent_type_id| {
            map.insert(
                AGENT_TYPE_ID_ANNOTATION_KEY.to_string(),
                agent_type_id.to_string(),
            );
        });
        Self(map)
    }

    pub fn new_sub_agent_owned(agent_type_id: &AgentTypeID) -> Self {
        Self(BTreeMap::from([
            (
                OWNED_BY_ANNOTATION_KEY.to_string(),
                OWNED_BY_SUB_AGENT.to_string(),
            ),
            (
                AGENT_TYPE_ID_ANNOTATION_KEY.to_string(),
                agent_type_id.to_string(),
            ),
        ]))
    }

    pub fn get(&self) -> BTreeMap<String, String> {
        self.0.clone()
    }
}

pub fn get_agent_type_id_value(annotations: &BTreeMap<String, String>) -> Option<&String> {
    annotations.get(AGENT_TYPE_ID_ANNOTATION_KEY)
}

pub fn get_owned_by_value(annotations: &BTreeMap<String, String>) -> Option<&String> {
    annotations.get(OWNED_BY_ANNOTATION_KEY)
}

pub fn is_owned_by_agent_control(annotations: &BTreeMap<String, String>) -> bool {
    get_owned_by_value(annotations).is_some_and(|v| v == OWNED_BY_AGENT_CONTROL)
}

pub fn is_owned_by_sub_agent(annotations: &BTreeMap<String, String>) -> bool {
    get_owned_by_value(annotations).is_some_and(|v| v == OWNED_BY_SUB_AGENT)
}
