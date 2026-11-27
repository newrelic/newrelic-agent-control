//! Names of the permanent Job Objects that track the executables of the sub-agents.

use crate::job_handle::JobObjectError;
use std::fmt::{Display, Formatter};

/// Name of a named Job Object tracking an executable of a sub-agent:
/// `Global\nr-ac-<agent_id>.<exec_id>`, in the namespace shared by all sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobName(String);

impl JobName {
    const PREFIX: &str = r"Global\nr-ac-";

    /// Returns and validates the name of the job tracking the executable `exec_id` of the sub-agent `agent_id`.
    pub fn for_executable(agent_id: &str, exec_id: &str) -> Result<Self, JobObjectError> {
        let agent_id_is_valid =
            !agent_id.is_empty() && !agent_id.contains('\\') && !agent_id.contains('.');
        let exec_id_is_valid = !exec_id.is_empty() && !exec_id.contains('\\');
        if !agent_id_is_valid || !exec_id_is_valid {
            return Err(JobObjectError(format!(
                "invalid JobObject name for agent '{agent_id}' and executable '{exec_id}'"
            )));
        }
        Ok(Self(format!("{}{agent_id}.{exec_id}", Self::PREFIX)))
    }
}

impl AsRef<str> for JobName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for JobName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::simple("nr-infra", "newrelic-infra", r"Global\nr-ac-nr-infra.newrelic-infra")]
    #[case::dotted_executable("nr-infra", "a.b", r"Global\nr-ac-nr-infra.a.b")]
    fn test_job_name_for_executable(
        #[case] agent_id: &str,
        #[case] exec_id: &str,
        #[case] expected: &str,
    ) {
        let name = JobName::for_executable(agent_id, exec_id).unwrap();
        assert_eq!(name.as_ref(), expected);
        assert_eq!(name.to_string(), expected);
    }

    #[rstest]
    #[case::empty_agent("", "exec")]
    #[case::agent_with_path_separator(r"nested\agent", "exec")]
    #[case::agent_with_dot("agent.x", "exec")]
    #[case::empty_executable("agent", "")]
    #[case::executable_with_path_separator("agent", r"nested\exec")]
    fn test_job_name_rejects_invalid_ids(#[case] agent_id: &str, #[case] exec_id: &str) {
        let err = JobName::for_executable(agent_id, exec_id).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("invalid JobObject name for agent '{agent_id}' and executable '{exec_id}'")
        );
    }
}
