//! On-host sub-agent: builder and supervisor that run agent executables and packages locally.

pub mod builder;
pub mod command;
pub mod supervisor;

#[cfg(test)]
pub(crate) mod test_utils {
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::agent_id::tests::UniqueAgentID;

    /// The process/logger threads don't inherit the test's `#[traced_test]` span (spans aren't
    /// propagated across `std::thread::spawn`), so `logs_contain` can't see their output. Read
    /// the shared log buffer directly instead, filtered to the lines emitted for `agent_id`.
    pub(crate) fn global_logs_lines(agent_id: UniqueAgentID) -> Vec<String> {
        let agent_id = AgentID::from(agent_id);
        String::from_utf8(tracing_test::internal::global_buf().lock().unwrap().clone())
            .unwrap()
            .lines()
            .filter(|l| {
                l.split_whitespace()
                    .any(|t| t == format!("agent_id={agent_id}"))
            })
            .map(String::from)
            .collect()
    }
}
