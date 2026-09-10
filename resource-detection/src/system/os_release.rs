//! Parses `/etc/os-release` to get the Linux distribution version.
//!
//! The file format is a simple `KEY=value` list (see `os-release(5)`), with values
//! optionally double-quoted. We only need `VERSION_ID`.

#[cfg(target_os = "linux")]
const OS_RELEASE_PATH: &str = "/etc/os-release";

/// Reads `/etc/os-release` and returns its `VERSION_ID` (e.g. "11"). Linux only;
/// always `None` on other targets, or if the file can't be read or has no
/// `VERSION_ID` line.
#[cfg(target_os = "linux")]
pub fn detect_os_version() -> Option<String> {
    let content = std::fs::read_to_string(OS_RELEASE_PATH).ok()?;
    parse_os_version(&content)
}

/// Parses `VERSION_ID` out of the contents of an os-release file. Exposed
/// separately from [`detect_os_version`] so it can be exercised against
/// fixture content without touching the filesystem.
///
/// Only ever called on Linux (see `detect_os_version` above), so this is dead
/// code on other targets from the compiler's point of view even though its
/// tests still run everywhere.
#[allow(dead_code)]
pub fn parse_os_version(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "VERSION_ID").then(|| value.trim().trim_matches('"').to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_debian_11() {
        let content = r#"PRETTY_NAME="Debian GNU/Linux 11 (bullseye)"
NAME="Debian GNU/Linux"
VERSION_ID="11"
VERSION="11 (bullseye)"
VERSION_CODENAME=bullseye
ID=debian
"#;
        assert_eq!(parse_os_version(content), Some("11".to_string()));
    }

    #[test]
    fn parses_unquoted_value() {
        let content = "NAME=Ubuntu\nVERSION_ID=22.04\nID=ubuntu\n";
        assert_eq!(parse_os_version(content), Some("22.04".to_string()));
    }

    #[test]
    fn missing_version_id_is_none() {
        assert_eq!(parse_os_version("ID=somedistro\n"), None);
    }

    #[test]
    fn empty_content_is_none() {
        assert_eq!(parse_os_version(""), None);
    }

    #[test]
    fn ignores_malformed_lines() {
        let content = "this line has no equals sign\nVERSION_ID=11\n";
        assert_eq!(parse_os_version(content), Some("11".to_string()));
    }
}
