//! Parses `/etc/os-release` to get the Linux distribution version and id.
//!
//! The file format is a simple `KEY=value` list (see `os-release(5)`), with values
//! optionally double-quoted. We only need `VERSION_ID` and `ID`.

#[cfg(target_os = "linux")]
const OS_RELEASE_PATH: &str = "/etc/os-release";

/// Reads `/etc/os-release` and returns its `VERSION_ID` (e.g. "11"). `None` if
/// the file can't be read or has no `VERSION_ID` line. Linux only: this
/// function doesn't exist on other targets (see `system::os_version` for the
/// cross-platform entry point).
#[cfg(target_os = "linux")]
pub fn detect_os_version() -> Option<String> {
    let content = std::fs::read_to_string(OS_RELEASE_PATH).ok()?;
    parse_os_version(&content)
}

/// Reads `/etc/os-release` and returns its `ID` (e.g. "debian", "ubuntu").
/// `None` if the file can't be read or has no `ID` line. Linux only: this
/// function doesn't exist on other targets (see `system::os_name` for the
/// cross-platform entry point).
#[cfg(target_os = "linux")]
pub fn detect_os_id() -> Option<String> {
    let content = std::fs::read_to_string(OS_RELEASE_PATH).ok()?;
    parse_os_id(&content)
}

/// Parses a single `KEY=value` field out of the contents of an os-release file.
fn parse_os_release_field(content: &str, field: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == field).then(|| value.trim().trim_matches('"').to_string())
    })
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
    parse_os_release_field(content, "VERSION_ID")
}

/// Parses `ID` out of the contents of an os-release file. `ID` is the
/// distro's short, machine-readable identifier (e.g. "debian", "ubuntu",
/// "opensuse-leap"), unlike `NAME`'s human-readable product string (e.g.
/// "Debian GNU/Linux"). Combined with `VERSION_ID`, it disambiguates
/// otherwise-colliding version numbers across distros (Debian 12 and
/// openSUSE Leap 12 both have `VERSION_ID=12`), without carrying the
/// marketing text a previous revision of this feature dropped as
/// unnecessary.
///
/// Exposed separately from [`detect_os_id`] so it can be exercised against
/// fixture content without touching the filesystem.
///
/// Only ever called on Linux (see `detect_os_id` above), so this is dead
/// code on other targets from the compiler's point of view even though its
/// tests still run everywhere.
#[allow(dead_code)]
pub fn parse_os_id(content: &str) -> Option<String> {
    parse_os_release_field(content, "ID")
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

    #[test]
    fn parses_debian_id() {
        let content = r#"PRETTY_NAME="Debian GNU/Linux 11 (bullseye)"
NAME="Debian GNU/Linux"
VERSION_ID="11"
VERSION="11 (bullseye)"
VERSION_CODENAME=bullseye
ID=debian
"#;
        assert_eq!(parse_os_id(content), Some("debian".to_string()));
    }

    #[test]
    fn parses_unquoted_id() {
        let content = "NAME=Ubuntu\nVERSION_ID=22.04\nID=ubuntu\n";
        assert_eq!(parse_os_id(content), Some("ubuntu".to_string()));
    }

    #[test]
    fn missing_id_is_none() {
        assert_eq!(parse_os_id("VERSION_ID=11\n"), None);
    }
}
