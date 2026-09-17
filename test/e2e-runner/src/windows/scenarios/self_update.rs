use crate::common::docker_hub::published_ac_tags;
use crate::common::oci::{OciRegistry, push_ac_package};
use crate::common::on_drop::CleanUp;
use crate::common::runtime::tokio_runtime;
use crate::common::test::{TestResult, retry_panic};
use crate::common::{InstallationArgs, RecipeData, config};
use crate::windows::install::{
    SERVICE_NAME, install_agent_control_from_recipe, install_latest_agent_control, tear_down_test,
};
use crate::windows::service::{STATUS_RUNNING, restart_service};
use crate::windows::{self};
use fake_opamp_server::{FakeServer, InstanceID};
use std::time::Duration;
use tracing::info;

const AGENT_VERSION_ATTR: &str = "agent.version";

pub fn test_self_update_from_latest_to_current(args: InstallationArgs) {
    info!("Starting self-update scenario");

    let registry = OciRegistry::start();
    let pushed_package = push_ac_package(&args);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    info!("Fake OpAMP server started at {}", opamp_server.endpoint());

    let _clean_up = CleanUp::new(tear_down_test);

    install_latest_agent_control(&RecipeData {
        args: args.clone(),
        ..Default::default()
    });

    let self_update_config = format!(
        r#"
fleet_control:
  endpoint: {}
  signature_validation:
    public_key_server_url: {}
oci:
  registry: {}
log:
  file: 
    enabled: true
  level: debug
self_update:
  enabled: true
  signature_verification_enabled: true
  package:
    download:
      oci:
        repository: test
        public_key_url: {}
"#,
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        registry.url(),
        pushed_package.jwks_url,
    );
    config::update_config(windows::DEFAULT_AC_CONFIG_PATH, &self_update_config);

    restart_service(SERVICE_NAME, STATUS_RUNNING);
    info!("AC service restarted with fleet and self-update configuration");

    let instance_id = retry_panic(
        20,
        Duration::from_secs(2),
        "AC connecting to OpAMP server",
        || {
            opamp_server
                .find_agent_control_instance()
                .map_err(|e| e.into())
        },
    );
    info!("AC connected to fake OpAMP server");

    let initial_version = retry_panic(
        30,
        Duration::from_secs(2),
        "reading initial agent.version attribute",
        || -> TestResult<_> {
            opamp_server
                .get_identifying_attr_value(instance_id.clone(), AGENT_VERSION_ATTR)
                .ok_or_else(|| "agent.version attribute not set yet".into())
        },
    );
    info!(
        version = initial_version,
        "Verified initial AC version before self-update"
    );

    let new_version = pushed_package.reference.tag().unwrap();
    assert_ne!(
        initial_version, new_version,
        "initial and new version must differ for self-update to be meaningful"
    );

    let update_config = format!(
        r#"
version: "{new_version}"
agents: {{}}
"#
    );
    opamp_server.set_config_response(instance_id.clone(), update_config);
    info!(tag = new_version, "Sent self-update remote config");

    info!("Verifying remote config status is Applied");
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

    info!("Verifying agent.version attribute reflects the updated version");
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
            if reported_version == new_version {
                Ok(())
            } else {
                Err(format!("expected version {new_version}, got {reported_version}").into())
            }
        },
    );
    info!(version = new_version, "AC version updated successfully");

    info!("Self-update test completed successfully");
}

pub fn test_self_update_rollback(args: InstallationArgs) {
    info!("Starting self-update rollback scenario");

    let _clean_up = CleanUp::new(tear_down_test);
    let [version_a, version_b] = two_distinct_published_versions();
    let (mut opamp_server, instance_id, initial_version) = start_self_update_test(&args);
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

fn two_distinct_published_versions() -> [String; 2] {
    let mut tags = retry_panic(
        10,
        Duration::from_secs(2),
        "fetching published AC tags from Docker Hub",
        || published_ac_tags(2),
    );
    [tags.remove(0), tags.remove(0)]
}

fn start_self_update_test(args: &InstallationArgs) -> (FakeServer, InstanceID, String) {
    let opamp_server = FakeServer::start(tokio_runtime().handle());
    info!("Fake OpAMP server started at {}", opamp_server.endpoint());

    let recipe_data = RecipeData {
        args: args.clone(),
        ..Default::default()
    };

    install_agent_control_from_recipe(&recipe_data);

    let self_update_config = format!(
        r#"
agents: {{}}
fleet_control:
  endpoint: {}
  signature_validation:
    public_key_server_url: {}
log:
  file:
    enabled: true
  level: debug
"#,
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
    );
    config::update_config(windows::DEFAULT_AC_CONFIG_PATH, &self_update_config);

    restart_service(SERVICE_NAME, STATUS_RUNNING);
    info!("AC service restarted with fleet and self-update configuration");

    let instance_id = retry_panic(
        20,
        Duration::from_secs(2),
        "AC connecting to OpAMP server",
        || {
            opamp_server
                .find_agent_control_instance()
                .map_err(|e| e.into())
        },
    );
    info!("AC connected to fake OpAMP server");

    let initial_version = retry_panic(
        30,
        Duration::from_secs(2),
        "reading initial agent.version attribute",
        || -> TestResult<_> {
            opamp_server
                .get_identifying_attr_value(instance_id.clone(), AGENT_VERSION_ATTR)
                .ok_or_else(|| "agent.version attribute not set yet".into())
        },
    );
    info!(
        version = initial_version,
        "Verified initial AC version before self-update"
    );

    (opamp_server, instance_id, initial_version)
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
