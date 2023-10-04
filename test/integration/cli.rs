use std::{fs::File, io::Write, path::PathBuf};

use assert_cmd::Command;
use predicates::prelude::predicate;
use tempfile::TempDir;

// when the TempDir is dropped, the temporal directory is removed, thus, the its
// ownership must remain on the parent function.
fn create_simple_config(dir: &TempDir) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let file_path = dir.path().join("static.yml");
    let mut file = File::create(&file_path)?;
    writeln!(file, "agents: {{}}")?;
    Ok(file_path)
}

#[test]
fn print_debug_info() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let file_path = create_simple_config(&dir)?;
    let mut cmd = Command::cargo_bin("newrelic-super-agent")?;
    cmd.arg("--config").arg(file_path).arg("--print-debug-info");
    cmd.assert().success();
    Ok(())
}

#[cfg(unix)]
#[test]
fn does_not_run_if_no_root() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let file_path = create_simple_config(&dir)?;
    let mut cmd = Command::cargo_bin("newrelic-super-agent")?;
    cmd.arg("--config").arg(file_path);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Program must run as root"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn runs_as_root() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    let dir = TempDir::new()?;
    let file_path = create_simple_config(&dir)?;

    let mut cmd = Command::cargo_bin("newrelic-super-agent")?;
    cmd.arg("--config").arg(file_path);
    // cmd_assert is not made for long running programs, so we kill it anyway after 1 second
    cmd.timeout(Duration::from_secs(1));
    // But in any case we make sure that it actually attempted to create the supervisor group,
    // so it works when the program is run as root
    cmd.assert()
        .failure()
        .stdout(predicate::str::contains("Creating the signal handler"))
        .stdout(predicate::str::contains("Creating the global context"))
        .stdout(predicate::str::contains("Starting the super agent"));
    // No supervisor group so we don't check for it.
    Ok(())
}
