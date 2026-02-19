use crate::agent_type::agent_type_id::AgentTypeID;
use std::collections::BTreeMap;

const AGENT_TYPE_ID_ANNOTATION_KEY: &str = "newrelic.io/agent-type-id";

/// Collection of annotations used to identify agent control resources.
#[derive(PartialEq, Default)]
pub struct Annotations(BTreeMap<String, String>);

impl Annotations {
    pub fn new_agent_type_id_annotation(agent_type: &AgentTypeID) -> Self {
        let mut annotations = Self::default();
        annotations.0.insert(
            AGENT_TYPE_ID_ANNOTATION_KEY.to_string(),
            agent_type.to_string(),
        );
        annotations
    }

    /// Adds extra annotations to the collection WITHOUT replacing existing ones.
    pub fn append_extra_annotations(&mut self, annotations: &BTreeMap<String, String>) {
        for (label, value) in annotations.iter() {
            self.0.entry(label.clone()).or_insert(value.clone());
        }
    }

    pub fn get(&self) -> BTreeMap<String, String> {
        self.0.clone()
    }
}

pub fn get_agent_type_id_value(annotations: &BTreeMap<String, String>) -> Option<&String> {
    annotations.get(AGENT_TYPE_ID_ANNOTATION_KEY)
}
