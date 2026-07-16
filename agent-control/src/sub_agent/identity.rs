//! Sub-agent identity: the pairing of an [AgentID] with its [AgentTypeID].

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{AGENT_CONTROL_NAMESPACE, AGENT_CONTROL_TYPE};
use crate::agent_type::agent_type_id::AgentTypeID;
use std::fmt::{Display, Formatter};

/// Attribute key used to identify an agent by its id.
pub const ID_ATTRIBUTE_NAME: &str = "agent_id";

const AC_AGENT_TYPE_VERSION: &str = "0.1.0";

// This could be SubAgentIdentity
/// Identifies a sub-agent by its id and agent type.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentIdentity {
    /// The agent's unique id.
    pub id: AgentID,
    /// The agent's type id (namespace, name, version).
    pub agent_type_id: AgentTypeID,
}

impl AgentIdentity {
    /// AC doesn't have a real identity as agent since there is no Agent type for it. In order to build one and
    /// make possible to reuse some components that are based on this we use a fake [AgentTypeID] for AC.
    pub fn new_agent_control_identity() -> Self {
        let ac_agent_type_id =
            format!("{AGENT_CONTROL_NAMESPACE}/{AGENT_CONTROL_TYPE}:{AC_AGENT_TYPE_VERSION}");
        Self::from((
            AgentID::AgentControl,
            // This is a safe unwrap because we are creating the AgentTypeID from a string that we know is valid.
            // Unit tests will catch any issues with the string format, before it gets to be released.
            AgentTypeID::try_from(ac_agent_type_id.as_str()).unwrap_or_else(|_| {
                panic!("Fail to create AC Agent Type ID from: {ac_agent_type_id}")
            }),
        ))
    }
}

impl From<(AgentID, AgentTypeID)> for AgentIdentity {
    fn from(value: (AgentID, AgentTypeID)) -> Self {
        AgentIdentity {
            id: value.0,
            agent_type_id: value.1,
        }
    }
}
impl From<(&AgentID, &AgentTypeID)> for AgentIdentity {
    fn from(value: (&AgentID, &AgentTypeID)) -> Self {
        AgentIdentity {
            id: value.0.clone(),
            agent_type_id: value.1.clone(),
        }
    }
}

impl Display for AgentIdentity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.agent_type_id, self.id)
    }
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use super::*;

    impl Default for AgentIdentity {
        fn default() -> Self {
            AgentIdentity {
                id: AgentID::try_from("default").unwrap(),
                agent_type_id: AgentTypeID::try_from("default/default:0.0.1").unwrap(),
            }
        }
    }

    #[test]
    fn test_new_agent_control_identity() {
        // Asserts that all fields are correctly set and this doesn't cause a panic
        let _ = AgentIdentity::new_agent_control_identity();
    }

    /// Guards the naming convention `ID_ATTRIBUTE_NAME` exists to enforce: every tracing span
    /// carrying an agent identity must use the field name `agent_id`, never the bare `id`. A
    /// stray literal `id = ...` field silently drops out of the "Recent logs"/"Recent spans"
    /// dashboard widgets, which filter on `agent_id` - this exact bug shipped once already and
    /// was only caught via a live dashboard report, not by any test.
    #[test]
    fn no_stray_id_named_span_fields() {
        use regex::Regex;
        use std::path::Path;

        // Matches `id = <expr>` inside an info_span!/#[instrument] field list, but not
        // `agent_id`, `package_id`, `exec_id`, `resource_id`, `guid`, etc. (anything where the
        // `id`/`guid` is preceded by a word character, i.e. is a suffix of a longer identifier).
        let offending_field = Regex::new(r"(?:^|[^A-Za-z0-9_])id\s*=").unwrap();
        let span_macro = Regex::new(r"info_span!|#\[instrument").unwrap();

        let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut violations = Vec::new();
        collect_violations(&src_dir, &mut violations, &span_macro, &offending_field);

        assert!(
            violations.is_empty(),
            "found span(s) using the bare `id` field instead of `agent_id`:\n{}",
            violations.join("\n")
        );
    }

    fn collect_violations(
        dir: &std::path::Path,
        violations: &mut Vec<String>,
        span_macro: &regex::Regex,
        offending_field: &regex::Regex,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_violations(&path, violations, span_macro, offending_field);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (lineno, line) in contents.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                if span_macro.is_match(line) && offending_field.is_match(line) {
                    violations.push(format!(
                        "{}:{}: {}",
                        path.display(),
                        lineno + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
}
