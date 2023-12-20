use crate::config::super_agent_configs::AgentID;
use std::collections::BTreeMap;

pub const MANAGED_BY_KEY: &str = "app.kubernetes.io/managed-by";
pub const MANAGED_BY_VAL: &str = "newrelic-super-agent";
pub const AGENT_ID_LABEL_KEY: &str = "newrelic.io/agent-id";

/// Collection of labels used to identify super agent resources.
#[derive(Default, PartialEq)]
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
    pub fn with_agent_id(mut self, agent_id: &AgentID) -> Self {
        self.0
            .insert(AGENT_ID_LABEL_KEY.to_string(), agent_id.get());
        self
    }

    /// Adds extra labels to the collection WITHOUT replacing existing ones.
    pub fn append_extra_labels(&mut self, labels: &BTreeMap<String, String>) {
        for (label, value) in labels.iter() {
            self.0.entry(label.clone()).or_insert(value.clone());
        }
    }

    pub fn get(&self) -> BTreeMap<String, String> {
        self.0.clone()
    }

    /// Prints a label selector that matches all labels in the set.
    pub fn selector(&self) -> String {
        let selector = self.0.iter().fold(String::new(), |acc, label| {
            format!("{acc}{}=={},", label.0, label.1)
        });

        selector.strip_suffix(',').unwrap_or(&selector).to_string()
    }
}

#[cfg(test)]
pub(crate) mod test {
    use std::collections::BTreeMap;

    use crate::config::super_agent_configs::AgentID;

    use super::{DefaultLabels, AGENT_ID_LABEL_KEY, MANAGED_BY_KEY, MANAGED_BY_VAL};

    #[test]
    fn test_selector() {
        let agent_id = &AgentID::new("test").unwrap();
        let labels = DefaultLabels::new().with_agent_id(agent_id);
        assert_eq!(
            format!("{MANAGED_BY_KEY}=={MANAGED_BY_VAL},{AGENT_ID_LABEL_KEY}=={agent_id}"),
            labels.selector()
        );
    }

    #[test]
    fn test_append_extra_labels() {
        let agent_id = &AgentID::new("test").unwrap();
        let mut labels = DefaultLabels::new().with_agent_id(agent_id);
        labels.append_extra_labels(&BTreeMap::from([
            (
                AGENT_ID_LABEL_KEY.to_string(),
                "will-not-be-override".to_string(),
            ),
            ("foo".to_string(), "bar".to_string()),
        ]));

        assert_eq!(
            labels.0.get(AGENT_ID_LABEL_KEY).unwrap(),
            &agent_id.to_string()
        );

        assert_eq!(labels.0.get("foo").unwrap(), "bar");
    }
}
