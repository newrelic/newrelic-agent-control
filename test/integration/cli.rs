use std::{fs::File, io::Write, path::PathBuf};

use assert_cmd::Command;
use predicates::prelude::predicate;

fn create_simple_config() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = assert_fs::TempDir::new()?;
    let file_path = dir.path().join("static.yml");
    let mut file = File::create(&file_path)?;
    writeln!(file, "agents: {{}}")?;
    Ok(file_path)
}

#[test]
fn print_debug_info() -> Result<(), Box<dyn std::error::Error>> {
    let file_path = create_simple_config()?;
    let mut cmd = Command::cargo_bin("main")?;
    cmd.arg("--config").arg(file_path).arg("--print-debug-info");
    cmd.assert().success();
    Ok(())
}

#[test]
fn does_not_run_if_no_root() -> Result<(), Box<dyn std::error::Error>> {
    let file_path = create_simple_config()?;
    let mut cmd = Command::cargo_bin("main")?;
    cmd.arg("--config").arg(file_path);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Program must run as root"));
    Ok(())
}
