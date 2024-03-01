use std::{fs::File, io::Write, path::PathBuf};

use assert_cmd::Command;
use predicates::prelude::predicate;
use tempfile::TempDir;

// when the TempDir is dropped, the temporal directory is removed, thus, the its
// ownership must remain on the parent function.
fn create_simple_config(dir: &TempDir, data: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let file_path = dir.path().join("static.yml");
    let mut file = File::create(&file_path)?;
    writeln!(file, "{data}")?;
    Ok(file_path)
}

#[test]
fn print_debug_info() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let file_path = create_simple_config(&dir, r"agents: {}")?;
    let mut cmd = Command::cargo_bin("newrelic-super-agent")?;
    cmd.arg("--config").arg(file_path).arg("--print-debug-info");
    cmd.assert().success();
    Ok(())
}

#[cfg(all(unix, feature = "onhost"))]
#[test]
fn does_not_run_if_no_root() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let file_path = create_simple_config(&dir, r"agents: {}")?;
    let mut cmd = Command::cargo_bin("newrelic-super-agent")?;
    cmd.arg("--config").arg(file_path);
    cmd.assert()
        .failure()
        .stdout(predicate::str::contains("Program must run as root"));
    Ok(())
}

#[cfg(all(unix, feature = "onhost"))]
#[test]
fn runs_as_root() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    let dir = TempDir::new()?;
    let file_path = create_simple_config(&dir, r"agents: {}")?;

    let mut cmd = Command::cargo_bin("newrelic-super-agent")?;
    cmd.arg("--config").arg(file_path);
    // cmd_assert is not made for long running programs, so we kill it anyway after 1 second
    cmd.timeout(Duration::from_secs(1));
    // But in any case we make sure that it actually attempted to create the supervisor group,
    // so it works when the program is run as root
    // The following regular expressions are used to ensure the logging format: 2024-02-16T07:49:44  INFO Creating the global context
    //   - (\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}) matches the timestamp format.
    // Any character match ".*" is used as the raw logging output contains the raw colors unicode
    // values: \u{1b}[2m2024\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \u{1b}[2mnewrelic_super_agent\u{1b}[0m\u{1b}[2m:\u{1b}[0m Creating the global context
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*New Relic Super Agent Version: .*, Rust Version: .*, GitCommit: .*, BuildDate: .*",
            )
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Starting the super agent",
            )
                .unwrap(),
        );
    // No supervisor group so we don't check for it.
    Ok(())
}

#[cfg(all(unix, feature = "onhost"))]
#[test]
fn custom_logging_format_as_root() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    let dir = TempDir::new()?;
    let file_path = create_simple_config(
        &dir,
        r#"
agents: {}
log:
  format:
    target: true
    timestamp: "%Y"
"#,
    )?;

    let mut cmd = Command::cargo_bin("newrelic-super-agent")?;
    cmd.arg("--config").arg(file_path);
    // cmd_assert is not made for long running programs, so we kill it anyway after 1 second
    cmd.timeout(Duration::from_secs(1));
    // But in any case we make sure that it actually attempted to create the supervisor group,
    // so it works when the program is run as root
    // The following regular expressions are used to ensure the logging format: 2024 INFO Creating the global context
    //   - (\d{4}) matches the timestamp format.
    //   - newrelic_super_agent as the target value
    // Any character match ".*" is used as the raw logging output contains the raw colors unicode
    // values: \u{1b}[2m2024\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \u{1b}[2mnewrelic_super_agent\u{1b}[0m\u{1b}[2m:\u{1b}[0m Creating the global context
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}).*INFO.*New Relic Super Agent Version: .*, Rust Version: .*, GitCommit: .*, BuildDate: .*",
            )
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}).*INFO.*newrelic_super_agent.*Starting the super agent",
            )
                .unwrap(),
        );
    // No supervisor group so we don't check for it.
    Ok(())
}
