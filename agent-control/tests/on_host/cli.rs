use crate::common::retry::retry;
use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
};
use newrelic_agent_control::on_host::file_store::build_config_name;
use predicates::prelude::predicate;
use std::error::Error;
use std::fs::create_dir_all;
use std::time::Duration;
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

pub fn create_temp_file(
    dir: &Path,
    file_name: &str,
    data: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    create_dir_all(dir)?;
    let file_path = dir.join(file_name);
    let mut file = File::create(&file_path)?;
    writeln!(file, "{data}")?;
    Ok(file_path)
}

pub fn cmd_with_config_file(local_dir: &Path) -> Command {
    let mut cmd = cargo_bin_cmd!("newrelic-agent-control");
    cmd.arg("--local-dir").arg(local_dir);
    // cmd_assert is not made for long running programs, so we kill it anyway after 1 second
    cmd.timeout(Duration::from_secs(10));
    cmd
}

#[test]
fn print_debug_info() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let _file_path = create_temp_file(dir.path(), FOLDER_NAME_LOCAL_DATA, r"agents: {}")?;
    let mut cmd = cargo_bin_cmd!("newrelic-agent-control");
    cmd.arg("--local-dir")
        .arg(dir.path())
        .arg("--print-debug-info");
    cmd.assert().success();
    Ok(())
}

#[cfg(target_family = "unix")]
#[test]
fn does_not_run_if_no_root() -> Result<(), Box<dyn std::error::Error>> {
    use newrelic_agent_control::on_host::file_store::build_config_name;

    let dir = TempDir::new()?;
    let _file_path = create_temp_file(
        &dir.path()
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID),
        build_config_name(STORE_KEY_LOCAL_DATA_CONFIG).as_str(),
        r"agents: {}",
    )?;
    let mut cmd = cargo_bin_cmd!("newrelic-agent-control");
    cmd.arg("--local-dir").arg(dir.path());
    cmd.assert()
        .failure()
        .stdout(predicate::str::contains("Program must run as root"));
    Ok(())
}

#[cfg(target_family = "unix")]
#[test]
fn basic_startup() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    use crate::on_host::logging::level::TIME_FORMAT;

    let dir = TempDir::new()?;
    let _file_path = create_temp_file(
        &dir.path()
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID),
        build_config_name(STORE_KEY_LOCAL_DATA_CONFIG).as_str(),
        r#"
agents: {}
server:
  enabled: false
logs:
  format:
    formatter: pretty
"#,
    )?;

    let mut cmd = cargo_bin_cmd!("newrelic-agent-control");
    cmd.arg("--local-dir").arg(dir.path());
    // cmd_assert is not made for long running programs, so we kill it anyway after 1 second
    cmd.timeout(Duration::from_secs(1));
    // But in any case we make sure that it actually attempted to create the supervisor group,
    // so it works when the program is run as root
    // The following regular expressions are used to ensure the logging format: 2024-02-16T07:49:44  INFO Creating the global context
    //   - (\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}) matches the timestamp format.
    // Any character match ".*" is used as the raw logging output contains the raw colors unicode
    // values: \u{1b}[2m2024\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \u{1b}[2mnewrelic_agent_control\u{1b}[0m\u{1b}[2m:\u{1b}[0m Creating the global context
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned()
                    + "INFO.*New Relic Agent Control Version: .*, Rust Version: .*, GitCommit: .*",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting NewRelic Agent Control",
            )
            .unwrap(),
        );
    // No supervisor group so we don't check for it.
    Ok(())
}

#[cfg(target_family = "unix")]
#[test]
fn custom_logging_format() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    let dir = TempDir::new()?;
    let _file_path = create_temp_file(
        &dir.path()
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID),
        build_config_name(STORE_KEY_LOCAL_DATA_CONFIG).as_str(),
        r#"
agents: {}
log:
  format:
    target: true
    timestamp: "%Y"
    formatter: pretty
server:
  enabled: false
"#,
    )?;

    let mut cmd = cargo_bin_cmd!("newrelic-agent-control");
    cmd.arg("--local-dir").arg(dir.path());
    // cmd_assert is not made for long running programs, so we kill it anyway after 1 second
    cmd.timeout(Duration::from_secs(1));
    // But in any case we make sure that it actually attempted to create the supervisor group,
    // so it works when the program is run as root
    // The following regular expressions are used to ensure the logging format: 2024 INFO Creating the global context
    //   - (\d{4}) matches the timestamp format.
    //   - newrelic_agent_control as the target value
    // Any character match ".*" is used as the raw logging output contains the raw colors unicode
    // values: \u{1b}[2m2024\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \u{1b}[2mnewrelic_agent_control\u{1b}[0m\u{1b}[2m:\u{1b}[0m Creating the global context
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}).*INFO.*New Relic Agent Control Version: .*, Rust Version: .*, GitCommit: .*",
            )
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}).*INFO.*newrelic_agent_control.*Starting NewRelic Agent Control",
            )
                .unwrap(),
        );
    // No supervisor group so we don't check for it.
    Ok(())
}

#[cfg(target_family = "unix")]
#[test]
#[ignore = "requires root"]
fn custom_directory_overrides_as_root() -> Result<(), Box<dyn std::error::Error>> {
    use assert_cmd::assert::OutputAssertExt;
    use httpmock::Method::POST;
    use httpmock::MockServer;

    let dir = TempDir::new()?;

    // simple mock that returns 200 so the agent can start and create the directories.
    let opamp_server = MockServer::start();
    let _opamp_server_mock = opamp_server.mock(|when, then| {
        when.method(POST).path("/");
        then.status(200);
    });

    let _config_path = create_temp_file(
        &dir.path()
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID),
        build_config_name(STORE_KEY_LOCAL_DATA_CONFIG).as_str(),
        format!(
            r#"
fleet_control:
  endpoint: "{}"
  signature_validation:
    enabled: false
host_id: "test-host-id"
log:
  level: info
  file:
    enabled: true
  format:
    formatter: pretty
agents: {{}}
server:
  enabled: false
    "#,
            opamp_server.url("/"),
        )
        .as_str(),
    )?;

    let tmpdir_path = dir.path();
    let tmpdir_remote = TempDir::new()?.path().join("test");
    let tmpdir_logs = TempDir::new()?.path().join("logs");

    let mut command = cargo_bin_cmd!("newrelic-agent-control");
    command
        .arg("--local-dir")
        .arg(tmpdir_path)
        .arg("--logs-dir")
        .arg(&tmpdir_logs)
        .arg("--remote-dir")
        .arg(&tmpdir_remote)
        .timeout(Duration::from_secs(10));

    // Ensure AC reaches a certain point in execution by checking the logs
    let _output = command.output()?.assert().stdout(predicates::str::contains(
        "Agents supervisor runtime successfully started",
    ));

    retry(
        90,
        Duration::from_secs(1),
        || -> Result<(), Box<dyn Error>> {
            if tmpdir_remote.exists() && tmpdir_logs.exists() {
                Ok(())
            } else {
                Err("Directories not created yet".into())
            }
        },
    );

    Ok(())
}

#[cfg(target_family = "unix")]
#[test]
fn runs_with_no_config() -> Result<(), Box<dyn std::error::Error>> {
    use std::{env, time::Duration};

    let dir = tempfile::tempdir()?;
    let mut cmd = cargo_bin_cmd!("newrelic-agent-control");
    cmd.arg("--local-dir").arg(dir.path());

    // We set the environment variable with the `__` separator which will create the nested
    // configs appropriately.
    let env_var_name = "NR_AC_AGENTS__ROLLDICE__AGENT_TYPE";
    unsafe { env::set_var(env_var_name, "namespace/com.newrelic.infrastructure:0.0.2") };

    // cmd_assert is not made for long running programs, so we kill it anyway after 1 second
    cmd.timeout(Duration::from_secs(1));
    // But in any case we make sure that it actually attempted to create the supervisor group,
    // so it works when the program is run as root
    // The following regular expressions are used to ensure the logging format: 2024-02-16T07:49:44  INFO Creating the global context
    //   - (\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}) matches the timestamp format.
    // Any character match ".*" is used as the raw logging output contains the raw colors unicode
    // values: \u{1b}[2m2024\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \u{1b}[2mnewrelic_agent_control\u{1b}[0m\u{1b}[2m:\u{1b}[0m Creating the global context
    cmd.assert()
        .failure()
        .stdout(predicate::str::contains(format!(
            "could not read Agent Control config from {}",
            dir.path().to_string_lossy()
        )));

    // Env cleanup
    unsafe { env::remove_var(env_var_name) };

    // No supervisor group, so we don't check for it.
    Ok(())
}
