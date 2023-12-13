use crate::config::super_agent_configs::AgentID;
use std::collections::BTreeMap;

pub const MANAGED_BY_KEY: &str = "app.kubernetes.io/managed-by";
pub const MANAGED_BY_VAL: &str = "newrelic-super-agent";
pub const AGENT_ID_LABEL_KEY: &str = "app.kubernetes.io/agent-id";

/// Collection of labels used to identify super agent resources.
#[derive(Default)]
pub struct DefaultLabels(BTreeMap<String, String>);

impl DefaultLabels {
    /// Creates a new collection of default labels.
    pub fn new() -> Self {
        DefaultLabels(BTreeMap::from([(
            MANAGED_BY_KEY.to_string(),
            MANAGED_BY_VAL.to_string(),
        )]))
    }

    /// Adds the agent id label to the set.
    pub fn with_agent_id(mut self, agent_id: AgentID) -> Self {
        self.0
            .insert(AGENT_ID_LABEL_KEY.to_string(), agent_id.get());
        self
    }

    pub fn get(&self) -> BTreeMap<String, String> {
        self.0.clone()
    }

    /// Prints a label selector that matches all labels in the set.
    pub fn selector(&self) -> String {
        let mut selector = String::new();

        let mut iter = self.0.iter();

        if let Some((k, v)) = iter.next() {
            selector.push_str(format!("{k}=={v}").as_str());
        }

        for (k, v) in iter {
            selector.push_str(format!(",{k}=={v}").as_str());
        }

        selector
    }
}

#[cfg(test)]
pub(crate) mod test {
    use crate::config::super_agent_configs::AgentID;

    use super::{DefaultLabels, AGENT_ID_LABEL_KEY, MANAGED_BY_KEY, MANAGED_BY_VAL};

    #[test]
    fn selector() {
        let agent_id = AgentID::new("test").unwrap();
        let labels = DefaultLabels::new().with_agent_id(agent_id.clone());
        assert_eq!(
            format!("{AGENT_ID_LABEL_KEY}=={agent_id},{MANAGED_BY_KEY}=={MANAGED_BY_VAL}"),
            labels.selector()
        );
    }
}
