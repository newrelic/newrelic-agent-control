use crate::common::oci::{OciRegistry, PushedPackage};
use crate::common::on_drop::CleanUp;
use crate::common::runtime::tokio_runtime;
use crate::common::self_update::{
    AGENT_VERSION_ATTR, run_self_update_latest_to_current_scenario,
    run_self_update_rollback_scenario,
};
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

pub fn test_self_update_from_latest_to_current(args: InstallationArgs) {
    let _clean_up = CleanUp::new(tear_down_test);
    run_self_update_latest_to_current_scenario(args, start_self_update_from_latest_test);
}

fn start_self_update_from_latest_test(
    args: InstallationArgs,
    registry: &OciRegistry,
    pushed_package: &PushedPackage,
) -> (FakeServer, InstanceID, String) {
    let opamp_server = FakeServer::start(tokio_runtime().handle());
    info!("Fake OpAMP server started at {}", opamp_server.endpoint());

    install_latest_agent_control(&RecipeData {
        args,
        ..Default::default()
    });

    let self_update_config = format!(
        r#"
agents: {{}}
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

    (opamp_server, instance_id, initial_version)
}

pub fn test_self_update_rollback(args: InstallationArgs) {
    let _clean_up = CleanUp::new(tear_down_test);
    run_self_update_rollback_scenario(args, start_self_update_test);
}

fn start_self_update_test(args: InstallationArgs) -> (FakeServer, InstanceID, String) {
    let opamp_server = FakeServer::start(tokio_runtime().handle());
    info!("Fake OpAMP server started at {}", opamp_server.endpoint());

    let recipe_data = RecipeData {
        args,
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
