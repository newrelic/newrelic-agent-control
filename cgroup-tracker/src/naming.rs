//! Cgroup naming rules.
//!
//! A cgroup is a directory in the cgroup filesystem hierarchy, so creating a cgroup means
//! creating a directory and its name is a directory name. This holds for both cgroup v2 and v1
//! (where the tracker uses a single hierarchy), so the same rules apply to either. That is why a
//! [`CgroupName`] must be a single, valid path segment that doesn't exceed the filesystem limit
//! for a directory name ([`MAX_LEN`]).
//!
//! Names can't contain `.`. The kernel creates interface files such as `cgroup.procs` or
//! `memory.max` inside every cgroup directory, and a child cgroup can't share their name. All of
//! them have a `<prefix>.<file>` form, so excluding `.` rules out the clashes without keeping a
//! list of controllers. The exceptions are the cgroup v1 files in [`RESERVED_NAMES`].
//!
//! The tracker places all of its cgroups under a dedicated [`CGROUP_ROOT`] directory, relative to
//! the tracker base, so each name maps to `<base>/_nr-ac/<name>`:
//!
//! ```text
//! <base>/
//! └── _nr-ac/          CGROUP_ROOT, owned by the tracker
//!     ├── <name-a>/    one cgroup per CgroupName
//!     └── <name-b>/
//! ```
//!
//! Keeping every cgroup under a single subtree lets the tracker manage (and clean up) only what
//! it created, without touching cgroups owned by other tools such as systemd.

use crate::error::InvalidCgroupName;
use std::path::PathBuf;

/// Directory, relative to the tracker base, holding every cgroup created by the tracker.
pub(crate) const CGROUP_ROOT: &str = "_nr-ac";

/// Maximum length of a single cgroup directory name.
const MAX_LEN: usize = 255;

/// cgroup v1 interface files that don't contain a `.`. Every other interface file is named
/// `<controller>.<file>`, which can't clash with a name since `.` is not allowed.
const RESERVED_NAMES: &[&str] = &["tasks", "notify_on_release"];

/// Name of a cgroup created by the tracker, located at `_nr-ac/<name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CgroupName(String);

impl CgroupName {
    /// Path relative to the tracker base.
    pub(crate) fn relative_path(&self) -> PathBuf {
        PathBuf::from(CGROUP_ROOT).join(&self.0)
    }
}

/// Validates the name. It must be made of ASCII alphanumerics, `-` and `_`, can't be empty and
/// can't be one of the [`RESERVED_NAMES`].
impl TryFrom<String> for CgroupName {
    type Error = InvalidCgroupName;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        let maybe_error = match name.as_str() {
            "" => Some("name is empty"),
            n if n.len() > MAX_LEN => Some("name is too long"),
            n if !n.chars().all(is_valid_char) => {
                Some("only ASCII alphanumerics, `-` and `_` are allowed")
            }
            n if RESERVED_NAMES.contains(&n) => Some("name is reserved for kernel interface files"),
            _ => None,
        };
        match maybe_error {
            None => Ok(Self(name)),
            Some(reason) => Err(InvalidCgroupName { name, reason }),
        }
    }
}

impl TryFrom<&str> for CgroupName {
    type Error = InvalidCgroupName;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        Self::try_from(name.to_string())
    }
}

impl AsRef<str> for CgroupName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CgroupName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_valid_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use rstest::rstest;

    #[rstest]
    #[case::plain("agent", "_nr-ac/agent")]
    #[case::composed("my-agent_1_exec", "_nr-ac/my-agent_1_exec")]
    #[case::controller_name("memory", "_nr-ac/memory")]
    #[case::reserved_name_inside("tasks-1", "_nr-ac/tasks-1")]
    fn valid_names(#[case] name: &str, #[case] expected: &str) {
        let name = CgroupName::try_from(name).unwrap();
        assert_eq!(name.relative_path(), PathBuf::from(expected));
        assert_eq!(name.to_string(), name.as_ref());
    }

    #[rstest]
    #[case::empty("")]
    #[case::dot(".")]
    #[case::dot_dot("..")]
    #[case::dot_inside("a.b")]
    #[case::kernel_file("cgroup.procs")]
    #[case::controller_file("memory.max")]
    #[case::slash("agent/exec")]
    #[case::space("my agent")]
    #[case::non_ascii("agénte")]
    #[case::nul("a\0b")]
    #[case::too_long(&"a".repeat(MAX_LEN + 1))]
    #[case::v1_tasks("tasks")]
    #[case::v1_notify("notify_on_release")]
    fn invalid_names(#[case] name: &str) {
        assert_matches!(CgroupName::try_from(name), Err(InvalidCgroupName { .. }));
    }

    #[test]
    fn max_length_is_accepted() {
        CgroupName::try_from("a".repeat(MAX_LEN)).unwrap();
    }
}
