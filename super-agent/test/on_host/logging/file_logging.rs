use assert_cmd::Command;
use predicates::prelude::predicate;
use std::{fs::read_dir, path::Path, time::Duration};
use tempfile::TempDir;

const LOG_FILE_CONFIG: &str = "test/on_host/logging/configs/file_logging.yaml";
// const LOG_FILE_LOCATION: &str = "test/on_host/logging/test/logs";

fn cmd_with_config_file(file_path: &Path) -> Command {
    let mut cmd = Command::cargo_bin("newrelic-super-agent").unwrap();
    cmd.arg("--config").arg(file_path);
    // cmd_assert is not made for long running programs, so we kill it anyway after 1 second
    cmd.timeout(Duration::from_secs(1));
    cmd
}

#[test]
fn default_log_level_no_root() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("super_agent.log");

    let mut cmd = cmd_with_config_file(&file_path);

    // Expecting to fail as non_root
    // Asserting content is logged to stdout as well
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Creating the signal handler",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Creating the global context",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*ERROR.*Program must run as root",
            )
            .unwrap(),
        );

    // Let's wait for a second so the flushed contents arrive to the files
    std::thread::sleep(Duration::from_secs(1));

    // Now, we assert that the file(s) created are present and contain the expected content
    let dir: Vec<_> = read_dir(dir.path())
        .unwrap()
        // We unwrap each entry to be able to order it
        .map(|entry| entry.unwrap())
        .collect();
    // if sorting is needed, use
    // dir.sort_by_key(|f| f.path());

    // We append the contents of the files in order
    let mut actual = String::new();
    for file in dir {
        actual.push_str(&std::fs::read_to_string(file.path()).unwrap());
    }
    // We delete the created directory
    // std::fs::remove_dir_all(LOG_FILE_LOCATION).unwrap();

    assert!(actual.contains("INFO Creating the signal handler"));
    assert!(actual.contains("INFO Creating the global context"));
    assert!(actual.contains("ERROR Program must run as root"));
}

#[test]
fn default_log_level_as_root() {
    let mut cmd = cmd_with_config_file(Path::new(LOG_FILE_CONFIG));

    // Expecting to fail as non_root
    // Asserting content is logged to stdout as well
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Creating the signal handler",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Creating the global context",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Starting the super agent",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Starting the supervisor group",
            )
            .unwrap(),
        );

    // Let's wait for a second so the flushed contents arrive to the files
    std::thread::sleep(Duration::from_secs(1));

    // Now, we assert that the file(s) created are present and contain the expected content
    let dir: Vec<_> = read_dir(LOG_FILE_LOCATION)
        .unwrap()
        // We unwrap each entry to be able to order it
        .map(|entry| entry.unwrap())
        .collect();
    // if sorting is needed, use
    // dir.sort_by_key(|f| f.path());

    // We append the contents of the files in order
    let mut actual = String::new();
    for file in dir {
        actual.push_str(&std::fs::read_to_string(file.path()).unwrap());
    }
    // We delete the created directory
    std::fs::remove_dir_all(LOG_FILE_LOCATION).unwrap();

    assert!(actual.contains("INFO Creating the signal handler"));
    assert!(actual.contains("INFO Creating the global context"));
    assert!(actual.contains("INFO Starting the super agent"));
    assert!(actual.contains("INFO Starting the supervisor group"));
}
