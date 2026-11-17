use crate::error::InvalidCgroupName;
use std::path::PathBuf;

/// Directory, relative to the tracker base, holding every cgroup created by the tracker.
pub(crate) const CGROUP_ROOT: &str = "_nr-ac";

/// Maximum length of a single cgroup directory name (`NAME_MAX`).
const MAX_LEN: usize = 255;

/// Name of a cgroup created by the tracker, located at `_nr-ac/<name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CgroupName(String);

impl CgroupName {
    /// Path relative to the tracker base.
    pub(crate) fn relative_path(&self) -> PathBuf {
        PathBuf::from(CGROUP_ROOT).join(&self.0)
    }
}

/// Validates the name. It must be a single path segment made of ASCII alphanumerics, `-`, `_`
/// or `.`, and can't be empty, `.` or `..`.
impl TryFrom<String> for CgroupName {
    type Error = InvalidCgroupName;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        let maybe_error = match name.as_str() {
            "" => Some("name is empty"),
            "." | ".." => Some("name is a reserved path segment"),
            n if n.len() > MAX_LEN => Some("name is too long"),
            n if !n.chars().all(is_valid_char) => {
                Some("only ASCII alphanumerics, `-`, `_` and `.` are allowed")
            }
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
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use rstest::rstest;

    #[rstest]
    #[case::plain("agent", "_nr-ac/agent")]
    #[case::composed("my-agent_1.exec", "_nr-ac/my-agent_1.exec")]
    #[case::dots_inside("a..b", "_nr-ac/a..b")]
    fn valid_names(#[case] name: &str, #[case] expected: &str) {
        let name = CgroupName::try_from(name).unwrap();
        assert_eq!(name.relative_path(), PathBuf::from(expected));
        assert_eq!(name.to_string(), name.as_ref());
    }

    #[rstest]
    #[case::empty("")]
    #[case::dot(".")]
    #[case::dot_dot("..")]
    #[case::slash("agent/exec")]
    #[case::space("my agent")]
    #[case::non_ascii("agénte")]
    #[case::nul("a\0b")]
    #[case::too_long(&"a".repeat(MAX_LEN + 1))]
    fn invalid_names(#[case] name: &str) {
        assert_matches!(CgroupName::try_from(name), Err(InvalidCgroupName { .. }));
    }

    #[test]
    fn max_length_is_accepted() {
        CgroupName::try_from("a".repeat(MAX_LEN)).unwrap();
    }
}
