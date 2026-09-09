//! Parses `/etc/os-release` to get the Linux distribution name and version.
//!
//! The file format is a simple `KEY=value` list (see `os-release(5)`), with values
//! optionally double-quoted. We only need `NAME` and `VERSION_ID`.

/// Distro name and version parsed from `/etc/os-release`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsRelease {
    /// The `NAME` field, e.g. "Debian GNU/Linux".
    pub name: Option<String>,
    /// The `VERSION_ID` field, e.g. "11".
    pub version_id: Option<String>,
}

#[cfg(target_os = "linux")]
const OS_RELEASE_PATH: &str = "/etc/os-release";

/// Reads and parses `/etc/os-release`. Linux only; always `None` on other
/// targets. Returns `None` if the file can't be read, or a partially-populated
/// `OsRelease` if it's readable but missing one of the two fields.
#[cfg(target_os = "linux")]
pub fn detect_os_release() -> Option<OsRelease> {
    let content = std::fs::read_to_string(OS_RELEASE_PATH).ok()?;
    Some(parse_os_release(&content))
}

/// Linux only; always `None` on other targets.
#[cfg(not(target_os = "linux"))]
pub fn detect_os_release() -> Option<OsRelease> {
    None
}

/// Parses the contents of an os-release file. Exposed separately from
/// [`detect_os_release`] so it can be exercised against fixture content without
/// touching the filesystem.
pub fn parse_os_release(content: &str) -> OsRelease {
    let mut name = None;
    let mut version_id = None;

    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key.trim() {
            "NAME" => name = Some(value),
            "VERSION_ID" => version_id = Some(value),
            _ => {}
        }
    }

    OsRelease { name, version_id }
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
        let os_release = parse_os_release(content);
        assert_eq!(os_release.name, Some("Debian GNU/Linux".to_string()));
        assert_eq!(os_release.version_id, Some("11".to_string()));
    }

    #[test]
    fn parses_ubuntu_unquoted_values() {
        let content = "NAME=Ubuntu\nVERSION_ID=\"22.04\"\nID=ubuntu\n";
        let os_release = parse_os_release(content);
        assert_eq!(os_release.name, Some("Ubuntu".to_string()));
        assert_eq!(os_release.version_id, Some("22.04".to_string()));
    }

    #[test]
    fn missing_fields_are_none() {
        let os_release = parse_os_release("ID=somedistro\n");
        assert_eq!(os_release.name, None);
        assert_eq!(os_release.version_id, None);
    }

    #[test]
    fn empty_content_is_all_none() {
        let os_release = parse_os_release("");
        assert_eq!(
            os_release,
            OsRelease {
                name: None,
                version_id: None
            }
        );
    }

    #[test]
    fn ignores_malformed_lines() {
        let content = "this line has no equals sign\nNAME=Debian\n";
        let os_release = parse_os_release(content);
        assert_eq!(os_release.name, Some("Debian".to_string()));
    }
}
