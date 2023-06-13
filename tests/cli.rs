use std::{fs::File, io::Write};

use assert_cmd::Command;
use predicates::prelude::predicate;

#[test]
fn print_debug_info() -> Result<(), Box<dyn std::error::Error>> {
    let dir = assert_fs::TempDir::new()?;
    let file_path = dir.path().join("static.yml");
    let mut file = File::create(&file_path)?;
    writeln!(file, "agents: {{}}")?;

    let mut cmd = Command::cargo_bin("main")?;
    cmd.arg("--config").arg(file_path).arg("--print-debug-info");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Error: Debug"));

    Ok(())
}
