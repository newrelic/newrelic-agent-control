//! Self-update scenarios shared by the Linux and Windows e2e runners.

use crate::common::InstallationArgs;
use crate::common::docker_hub::published_ac_tags;
use crate::common::oci::{OciRegistry, PushedPackage, push_ac_package};
use crate::common::test::retry_panic;
use fake_opamp_server::{FakeServer, InstanceID};
use std::time::Duration;
use tracing::info;

pub const AGENT_VERSION_ATTR: &str = "agent.version";

/// Self-updates from a freshly-installed latest published version to the current branch build.
pub fn run_self_update_latest_to_current_scenario(
    args: InstallationArgs,
    start_test: impl FnOnce(
        InstallationArgs,
        &OciRegistry,
        &PushedPackage,
    ) -> (FakeServer, InstanceID, String),
) {
    info!("Starting self-update scenario");

    let registry = OciRegistry::start();
    let pushed_package = push_ac_package(&args);

    let (mut opamp_server, instance_id, initial_version) =
        start_test(args, &registry, &pushed_package);

    let new_version = pushed_package.reference.tag().unwrap();
    assert_ne!(
        initial_version, new_version,
        "initial and new version must differ for self-update to be meaningful"
    );

    trigger_update_and_wait(&mut opamp_server, &instance_id, new_version);

    info!("Self-update test completed successfully");
}

/// Self-updates to a published version, then another, then rolls back.
pub fn run_self_update_rollback_scenario(
    args: InstallationArgs,
    start_test: impl FnOnce(InstallationArgs) -> (FakeServer, InstanceID, String),
) {
    info!("Starting self-update rollback scenario");

    let current_version = args.agent_control_version.clone();
    let (mut opamp_server, instance_id, initial_version) = start_test(args);
    let [version_a, version_b] = two_latest_published_versions(&current_version);
    assert_ne!(
        initial_version, version_a,
        "initial and target version must differ for self-update to be meaningful"
    );

    trigger_update_and_wait(&mut opamp_server, &instance_id, &version_a);
    trigger_update_and_wait(&mut opamp_server, &instance_id, &version_b);
    info!("Rolling back to the previously-installed version");
    trigger_update_and_wait(&mut opamp_server, &instance_id, &version_a);

    info!("Self-update rollback test completed successfully");
}

/// Returns `[older, newer]`, so rolling back to the first element decreases the version.
fn two_latest_published_versions(current_version: &str) -> [String; 2] {
    let tags = retry_panic(
        10,
        Duration::from_secs(2),
        "fetching published AC tags from Docker Hub",
        || published_ac_tags(3),
    );
    let mut tags: Vec<String> = tags
        .into_iter()
        .filter(|tag| tag != current_version)
        .collect();
    let (newer, older) = (tags.remove(0), tags.remove(0));
    [older, newer]
}

fn trigger_update_and_wait(
    opamp_server: &mut FakeServer,
    instance_id: &InstanceID,
    target_version: &str,
) {
    let update_config = format!(
        r#"
version: "{target_version}"
agents: {{}}
"#
    );
    opamp_server.set_config_response(instance_id.clone(), update_config);
    info!(tag = target_version, "Sent self-update remote config");

    retry_panic(
        120,
        Duration::from_secs(2),
        "waiting for remote config Applied status",
        || {
            opamp_server
                .is_config_status_applied(instance_id.clone())
                .map_err(|e| e.into())
        },
    );

    retry_panic(
        120,
        Duration::from_secs(2),
        "verifying updated agent.version attribute",
        || {
            let Some(reported_version) =
                opamp_server.get_identifying_attr_value(instance_id.clone(), AGENT_VERSION_ATTR)
            else {
                return Err("agent.version attribute not set yet".into());
            };
            if reported_version == target_version {
                Ok(())
            } else {
                Err(format!("expected version {target_version}, got {reported_version}").into())
            }
        },
    );
    info!(version = target_version, "AC version updated successfully");
}
