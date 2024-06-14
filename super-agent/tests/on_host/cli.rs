use std::{fs::File, io::Write, path::PathBuf};

use assert_cmd::Command;
use predicates::prelude::predicate;
use tempfile::TempDir;

// when the TempDir is dropped, the temporal directory is removed, thus, the its
// ownership must remain on the parent function.
pub fn create_temp_file(
    dir: &TempDir,
    file_name: &str,
    data: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let file_path = dir.path().join(file_name);
    std::fs::create_dir_all(file_path.parent().unwrap())?;
    let mut file = File::create(&file_path)?;
    writeln!(file, "{data}")?;
    Ok(file_path)
}

#[test]
fn print_debug_info() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let file_path = create_temp_file(&dir, "static.yml", r"agents: {}")?;
    let mut cmd = Command::cargo_bin("newrelic-super-agent")?;
    cmd.arg("--config").arg(file_path).arg("--print-debug-info");
    cmd.assert().success();
    Ok(())
}

#[cfg(unix)]
#[test]
fn does_not_run_if_no_root() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let file_path = create_temp_file(&dir, "static.yml", r"agents: {}")?;
    let mut cmd = Command::cargo_bin("newrelic-super-agent")?;
    cmd.arg("--config").arg(file_path);
    cmd.assert()
        .failure()
        .stdout(predicate::str::contains("Program must run as root"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn runs_as_root() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    use crate::on_host::logging::level::TIME_FORMAT;

    let dir = TempDir::new()?;
    let file_path = create_temp_file(&dir, "static.yml", r"agents: {}")?;

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
                TIME_FORMAT.to_owned() + "INFO.*New Relic Super Agent Version: .*, Rust Version: .*, GitCommit: .*, BuildDate: .*",
            )
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting NewRelic Super Agent",
            )
                .unwrap(),
        );
    // No supervisor group so we don't check for it.
    Ok(())
}

#[cfg(unix)]
#[test]
fn custom_logging_format_as_root() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    let dir = TempDir::new()?;
    let file_path = create_temp_file(
        &dir,
        "static.yml",
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
                r".*(\d{4}).*INFO.*newrelic_super_agent.*Starting NewRelic Super Agent",
            )
                .unwrap(),
        );
    // No supervisor group so we don't check for it.
    Ok(())
}

#[cfg(unix)]
#[test]
fn custom_directory_as_root() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    let dir = TempDir::new()?;
    let _agent_type_def = create_temp_file(
        &dir,
        "nrsa_local/dynamic-agent-type.yaml",
        r#"
namespace: newrelic
name: com.newrelic.test-agent
version: 0.0.1
variables:
  on_host:
    message:
      description: "Message to repeatedly output"
      type: string
      required: false
      default: "yes"
    file_logging:
      description: "Enable file logging"
      type: bool
      required: false
      default: false
deployment:
  on_host:
    enable_file_logging: "${nr-var:file_logging}"
    executables:
      - path: /usr/bin/yes
        args: "${nr-var:message}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay: 20s
"#,
    );
    let _values_file = create_temp_file(
        &dir,
        "nrsa_local/fleet/agents.d/test-agent/values/values.yaml",
        r#"
message: "test yes"
file_logging: true
"#,
    );
    let config_path = create_temp_file(
        &dir,
        "static.yml",
        r#"
log:
  level: debug
  file:
    enable: true
agents:
  test-agent:
    agent_type: newrelic/com.newrelic.test-agent:0.0.1
"#,
    )?;

    let tmpdir_path = dir.path();
    let mut cmd = Command::cargo_bin("newrelic-super-agent")?;
    cmd.arg("--config")
        .arg(config_path)
        .arg("--debug")
        .arg(tmpdir_path);
    // cmd_assert is not made for long running programs, so we kill it anyway after 3 seconds
    cmd.timeout(Duration::from_secs(5));
    // But in any case we make sure that it actually attempted to create the supervisor group,
    // so it works when the program is run as root
    cmd.assert().failure();

    // Assert the directory structure has been created
    let remote_path = tmpdir_path.join("nrsa_remote");
    let logs_path = tmpdir_path.join("nrsa_logs");

    assert!(remote_path.exists());
    assert!(logs_path.exists());

    Ok(())
}

#[cfg(unix)]
#[test]
fn custom_directory_overrides_as_root() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    let dir = TempDir::new()?;
    let another_dir = TempDir::new()?;
    let _agent_type_def = create_temp_file(
        &dir,
        "nrsa_local/dynamic-agent-type.yaml",
        r#"
namespace: newrelic
name: com.newrelic.test-agent
version: 0.0.1
variables:
  on_host:
    message:
      description: "Message to repeatedly output"
      type: string
      required: false
      default: "yes"
    file_logging:
      description: "Enable file logging"
      type: bool
      required: false
      default: false
deployment:
  on_host:
    enable_file_logging: "${nr-var:file_logging}"
    executables:
      - path: /usr/bin/yes
        args: "${nr-var:message}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay: 20s
"#,
    );
    let _values_file = create_temp_file(
        &dir,
        "nrsa_local/fleet/agents.d/test-agent/values/values.yaml",
        r#"
message: "test yes"
file_logging: true
"#,
    );
    let config_path = create_temp_file(
        &dir,
        "static.yml",
        r#"
log:
  level: debug
  file:
    enable: true
agents:
  test-agent:
    agent_type: newrelic/com.newrelic.test-agent:0.0.1
"#,
    )?;

    let tmpdir_path = dir.path();
    let override_logs_path = another_dir.path().join("logs");

    let mut cmd = Command::cargo_bin("newrelic-super-agent")?;
    cmd.arg("--config")
        .arg(config_path)
        .arg("--debug")
        .arg(tmpdir_path)
        .arg("--logs-dir")
        .arg(&override_logs_path);
    // cmd_assert is not made for long running programs, so we kill it anyway after 3 seconds
    cmd.timeout(Duration::from_secs(5));
    // But in any case we make sure that it actually attempted to create the supervisor group,
    // so it works when the program is run as root
    cmd.assert().failure();

    // Assert the directory structure has been created
    let remote_path = tmpdir_path.join("nrsa_remote");

    assert!(remote_path.exists());
    assert!(override_logs_path.exists());

    Ok(())
}

#[cfg(unix)]
#[test]
fn runs_with_no_config_as_root() -> Result<(), Box<dyn std::error::Error>> {
    use std::{env, time::Duration};

    use crate::on_host::logging::level::TIME_FORMAT;

    let mut cmd = Command::cargo_bin("newrelic-super-agent")?;
    cmd.arg("--config").arg("non-existent-file.yaml");

    // We set the environment variable with the `__` separator which will create the nested
    // configs appropriately.
    let env_var_name = "NR_SA_AGENTS__ROLLDICE__AGENT_TYPE";
    env::set_var(
        env_var_name,
        "namespace/com.newrelic.infrastructure_agent:0.0.2",
    );

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
                TIME_FORMAT.to_owned() + "INFO.*New Relic Super Agent Version: .*, Rust Version: .*, GitCommit: .*, BuildDate: .*",
            )
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting NewRelic Super Agent",
            )
                .unwrap(),
        );

    // Env cleanup
    env::remove_var(env_var_name);

    // No supervisor group so we don't check for it.
    Ok(())
}
