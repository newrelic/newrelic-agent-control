//! Labels used to identify and select agent-control Kubernetes resources.
use crate::agent_control::agent_id::AgentID;
use std::collections::BTreeMap;

/// Label key indicating which application manages the resource.
pub const MANAGED_BY_KEY: &str = "app.kubernetes.io/managed-by";
/// Label value identifying agent-control as the managing application.
pub const MANAGED_BY_VAL: &str = "newrelic-agent-control";
/// Label key holding the agent id a resource belongs to.
pub const AGENT_ID_LABEL_KEY: &str = "newrelic.io/agent-id";
/// Label key indicating the source (local or remote) a version was set from.
pub const AGENT_CONTROL_VERSION_SET_FROM: &str = "newrelic.io/agent-control-version-set-from";
/// Label value indicating a locally-sourced value.
pub const LOCAL_VAL: &str = "local";
/// Label value indicating a remotely-sourced value.
pub const REMOTE_VAL: &str = "remote";

/// Collection of labels used to identify agent control resources.
#[derive(PartialEq)]
pub struct Labels(BTreeMap<String, String>);

impl Default for Labels {
    /// Creates a new collection of default labels.
    fn default() -> Self {
        Labels(BTreeMap::from([(
            MANAGED_BY_KEY.to_string(),
            MANAGED_BY_VAL.to_string(),
        )]))
    }
}
impl Labels {
    /// Adds the agent id label to the set.
    pub fn new(agent_id: &AgentID) -> Self {
        let mut labels = Self::default();
        labels
            .0
            .insert(AGENT_ID_LABEL_KEY.to_string(), agent_id.to_string());
        labels
    }

    /// Adds extra labels to the collection WITHOUT replacing existing ones.
    pub fn append_extra_labels(&mut self, labels: &BTreeMap<String, String>) {
        for (label, value) in labels.iter() {
            self.0.entry(label.clone()).or_insert(value.clone());
        }
    }

    /// Returns the labels as a key-value map.
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

/// returns true if labels indicates that is managed by the agentControl
pub fn is_managed_by_agent_control(labels: &BTreeMap<String, String>) -> bool {
    labels
        .get(MANAGED_BY_KEY)
        .is_some_and(|v| v == MANAGED_BY_VAL)
}

/// Returns the agent id label value, if present.
pub fn get_agent_id(labels: &BTreeMap<String, String>) -> Option<&String> {
    labels.get(AGENT_ID_LABEL_KEY)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{AGENT_ID_LABEL_KEY, Labels, MANAGED_BY_KEY, MANAGED_BY_VAL};
    use crate::agent_control::agent_id::AgentID;
    use std::collections::BTreeMap;

    #[test]
    fn test_selector() {
        let agent_id = &AgentID::try_from("test").unwrap();
        let labels = Labels::new(agent_id);
        assert_eq!(
            format!("{MANAGED_BY_KEY}=={MANAGED_BY_VAL},{AGENT_ID_LABEL_KEY}=={agent_id}"),
            labels.selector()
        );
    }

    #[test]
    fn test_append_extra_labels() {
        let agent_id = &AgentID::try_from("test").unwrap();
        let mut labels = Labels::new(agent_id);
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
