use crate::common::{
    agent_control::start_agent_control_with_custom_config, runtime::tokio_runtime,
};
use crate::on_host::tools::config::create_agent_control_config;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::defaults::AGENT_FILESYSTEM_FOLDER_NAME;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use std::time::Duration;
use tempfile::tempdir;

/// On startup, Agent Control reclaims the filesystem dirs of any sub-agent that is no longer in
/// the configured agents map (e.g. an agent removed from fleet config while AC was stopped). The
/// dir of any agent that *is* configured stays untouched.
#[test]
fn stale_agent_filesystem_cleanup_on_startup() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let tempdir = tempdir().expect("failed to create temp dir");
    let local_dir = tempdir.path().join("local");
    let remote_dir = tempdir.path().join("remote");

    // Pre-populate the filesystem dir with two sub-agent directories.
    let fs_base = remote_dir.join(AGENT_FILESYSTEM_FOLDER_NAME);
    let orphan_dir = fs_base.join("orphan-agent");
    let kept_dir = fs_base.join("configured-agent");
    std::fs::create_dir_all(orphan_dir.join("nested")).unwrap();
    std::fs::write(orphan_dir.join("nested/a.txt"), "stale").unwrap();
    std::fs::create_dir_all(&kept_dir).unwrap();
    std::fs::write(kept_dir.join("placeholder.txt"), "placeholder").unwrap();

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        "{}".to_string(),
        local_dir.to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.to_path_buf(),
        remote_dir: remote_dir.to_path_buf(),
        log_dir: local_dir.to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    // The orphan dir must be gone after AC startup. Use a small retry to allow the start-up
    // tasks to land before asserting.
    crate::common::retry::retry(30, Duration::from_secs(1), || {
        if orphan_dir.exists() {
            return Err(
                format!("orphan dir still present after AC startup: {orphan_dir:?}").into(),
            );
        }
        Ok(())
    });
}
