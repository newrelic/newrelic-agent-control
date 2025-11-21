use crate::on_host::cli::cmd_with_config_file;
use newrelic_agent_control::{
    agent_control::defaults::{
        AGENT_CONTROL_ID, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
    },
    on_host::file_store::build_config_name,
};
use predicates::prelude::predicate;
use tempfile::TempDir;

const EMPTY_CONFIG: &str = "# Empty config\nagents: {}";

const TRACE_LEVEL_CONFIG: &str = "agents: {}\nlog:\n  level: trace";

pub(crate) const TIME_FORMAT: &str = r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*";

#[cfg(all(target_family = "unix", not(feature = "disable-asroot")))]
#[test]
fn default_log_level_no_root() {
    let dir = TempDir::new().unwrap();

    let config_path = dir
        .path()
        .join(FOLDER_NAME_LOCAL_DATA)
        .join(AGENT_CONTROL_ID);

    std::fs::create_dir_all(&config_path).unwrap();

    std::fs::write(
        config_path.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
        EMPTY_CONFIG,
    )
    .unwrap();

    let mut cmd = cmd_with_config_file(dir.path());
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
                TIME_FORMAT.to_owned() + "ERROR.*Program must run with elevated permissions",
            )
            .unwrap(),
        );
}

#[test]
#[ignore = "requires root"]
fn default_log_level_as_root() {
    let dir = TempDir::new().unwrap();

    let config_path = dir
        .path()
        .join(FOLDER_NAME_LOCAL_DATA)
        .join(AGENT_CONTROL_ID);

    std::fs::create_dir_all(&config_path).unwrap();

    std::fs::write(
        config_path.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
        EMPTY_CONFIG,
    )
    .unwrap();

    let mut cmd = cmd_with_config_file(dir.path());
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned()
                    + "INFO.*New Relic Agent Control Version: .*, Rust Version: .*, GitCommit: .*, Environment: host",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting NewRelic Agent Control",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting the agents supervisor runtime",
            )
            .unwrap(),
        );
}

#[cfg(all(target_family = "unix", not(feature = "disable-asroot")))]
#[test]
fn debug_log_level_no_root() {
    const DEBUG_LEVEL_CONFIG: &str = "agents: {}\nlog:\n  level: debug";

    let dir = TempDir::new().unwrap();

    let config_path = dir
        .path()
        .join(FOLDER_NAME_LOCAL_DATA)
        .join(AGENT_CONTROL_ID);

    std::fs::create_dir_all(&config_path).unwrap();

    std::fs::write(
        config_path.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
        DEBUG_LEVEL_CONFIG,
    )
    .unwrap();

    let mut cmd = cmd_with_config_file(dir.path());
    // Expecting to fail as non_root
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "DEBUG.*tracing_subscriber initialized successfully",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "ERROR.*Program must run with elevated permissions",
            )
            .unwrap(),
        );
}

#[test]
#[ignore = "requires root"]
fn trace_log_level_as_root() {
    let dir = TempDir::new().unwrap();

    let config_path = dir
        .path()
        .join(FOLDER_NAME_LOCAL_DATA)
        .join(AGENT_CONTROL_ID);

    std::fs::create_dir_all(&config_path).unwrap();

    std::fs::write(
        config_path.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
        TRACE_LEVEL_CONFIG,
    )
    .unwrap();

    let mut cmd = cmd_with_config_file(dir.path());
    // Expecting to fail as non_root
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "DEBUG.*tracing_subscriber initialized successfully",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting NewRelic Agent Control",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(TIME_FORMAT.to_owned() + "TRACE.*creating the signal handler")
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(TIME_FORMAT.to_owned() + "TRACE.*creating the global context")
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting the agents supervisor runtime",
            )
            .unwrap(),
        );
}
