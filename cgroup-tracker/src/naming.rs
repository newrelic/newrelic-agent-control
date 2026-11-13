use std::path::PathBuf;

/// Prefix of every cgroup created by the tracker.
pub(crate) const CGROUP_PREFIX: &str = "nr-ac-";

/// Identifies the cgroup of an executable of an agent.
///
/// The layout is nested (`nr-ac-<agent>/<executable>`) so ids containing `-` can't collide,
/// e.g. `("a-b", "c")` and `("a", "b-c")`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CgroupName {
    agent: String,
    executable: String,
}

impl CgroupName {
    /// Builds the name, replacing characters that are not valid in a cgroup directory name.
    pub fn new(agent: &str, executable: &str) -> Self {
        Self {
            agent: sanitize(agent),
            executable: sanitize(executable),
        }
    }

    /// Path relative to the tracker base.
    pub(crate) fn relative_path(&self) -> PathBuf {
        PathBuf::from(format!("{CGROUP_PREFIX}{}", self.agent)).join(&self.executable)
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn dashes_in_ids_do_not_collide() {
        assert_ne!(
            CgroupName::new("a-b", "c").relative_path(),
            CgroupName::new("a", "b-c").relative_path()
        );
    }

    #[rstest]
    #[case::plain("agent", "exec", "nr-ac-agent/exec")]
    #[case::sanitized("my agent", "ex/ec", "nr-ac-my_agent/ex_ec")]
    fn relative_path(#[case] agent: &str, #[case] exec: &str, #[case] expected: &str) {
        assert_eq!(
            CgroupName::new(agent, exec).relative_path(),
            PathBuf::from(expected)
        );
    }
}
