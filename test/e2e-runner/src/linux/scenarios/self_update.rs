use crate::common::docker_hub::latest_published_ac_tag;
use crate::common::oci::{OciRegistry, push_ac_package};
use crate::common::on_drop::CleanUp;
use crate::common::runtime::tokio_runtime;
use crate::common::test::{TestResult, retry_panic};
use crate::common::{InstallationArgs, RecipeData, config};
use crate::linux;
use crate::linux::install::{
    install_agent_control_from_recipe, install_latest_agent_control, tear_down_test,
};
use crate::linux::service::{STATUS_RUNNING, restart_service_and_wait};
use fake_opamp_server::FakeServer;
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
    config::update_config(linux::DEFAULT_AC_CONFIG_PATH, &self_update_config);

    restart_service_and_wait(linux::SERVICE_NAME, STATUS_RUNNING);
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

const NONEXISTENT_VERSION: &str = "999.999.999";
/// Budget for the second config to be applied while the first upgrade is in flight. Must be
/// comfortably smaller than the retry window so a frozen loop (old behavior) cannot finish in time.
const RESPONSIVE_BUDGET_SECS: i64 = 15;

/// The event loop must stay responsive while a self-update is in flight.
///
/// Steps:
///   1. push config1 (self-update to a nonexistent version) and wait until it is reported in flight,
///   2. push config2 (a benign, no-version config) while config1 is still retrying,
///   3. assert config2 reaches `Applied` well within the retry window.
pub fn test_event_loop_responsive_during_self_update(args: InstallationArgs) {
    info!("Starting event-loop-responsiveness during self-update scenario");

    let registry = OciRegistry::start();
    let pushed_package = push_ac_package(&args);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    info!("Fake OpAMP server started at {}", opamp_server.endpoint());

    let _clean_up = CleanUp::new(tear_down_test);

    install_latest_agent_control(&RecipeData {
        args: args.clone(),
        ..Default::default()
    });

    // `download_retry` gives a long, deterministic in-flight window: ~20 attempts × 5s backoff.
    // A failed download is retried the whole time, so config1 never completes during the test.
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
  download_retry:
    max_attempts: 20
    base_delay: 5s
    max_delay: 5s
    jitter: false
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
    config::update_config(linux::DEFAULT_AC_CONFIG_PATH, &self_update_config);

    restart_service_and_wait(linux::SERVICE_NAME, STATUS_RUNNING);
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

    let before_hash = opamp_server
        .get_remote_config_status(instance_id.clone())
        .map(|s| s.last_remote_config_hash);

    // config1: self-update to a version that isn't in the registry. Its download fails and is
    // retried for the whole window, so the attempt stays in flight.
    // It is reported Applying and never reaches a terminal state during the test.
    opamp_server.set_config_response(
        instance_id.clone(),
        format!("version: \"{NONEXISTENT_VERSION}\"\nagents: {{}}\n"),
    );
    info!(
        version = NONEXISTENT_VERSION,
        "Sent self-update remote config (config1)"
    );

    let config1_hash = retry_panic(
        30,
        Duration::from_secs(1),
        "config1 reported as in flight",
        || -> TestResult<Vec<u8>> {
            match opamp_server.get_remote_config_status(instance_id.clone()) {
                Some(s) if Some(&s.last_remote_config_hash) != before_hash.as_ref() => {
                    Ok(s.last_remote_config_hash)
                }
                other => Err(format!("config1 not reported yet: {other:?}").into()),
            }
        },
    );
    info!("config1 is in flight (self-update download retrying)");

    // config2: a benign, distinct, no-version config. `update()` returns NoOp for it, so it is
    // reported Applied immediately — IF the event loop is free to process it.
    opamp_server.set_config_response(instance_id.clone(), "agents: {}\n");
    info!("Sent second remote config (config2) while config1 is still in flight");

    retry_panic(
        RESPONSIVE_BUDGET_SECS,
        Duration::from_secs(1),
        "config2 applied while self-update in flight (event loop must not be frozen)",
        || -> TestResult<()> {
            let status = opamp_server
                .get_remote_config_status(instance_id.clone())
                .ok_or("no remote config status reported yet")?;
            let is_config2 = status.last_remote_config_hash != config1_hash;
            let is_applied = opamp_server
                .is_config_status_applied(instance_id.clone())
                .is_ok();
            if is_config2 && is_applied {
                Ok(())
            } else {
                Err(format!(
                    "config2 not applied yet (loop may be frozen): applied={is_applied}, is_config2={is_config2}"
                )
                .into())
            }
        },
    );

    info!("Event loop stayed responsive during self-update");
}

pub fn test_self_update_from_current_to_latest(args: InstallationArgs) {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    info!("Fake OpAMP server started at {}", opamp_server.endpoint());

    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

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
self_update:
  enabled: true
  # TODO remove below default config once AC >1.15.0 is released which will have
  # the serde_default for these.
  signature_verification_enabled: true
  package:
    download:
      oci:
        repository: "newrelic/agent-control-artifacts"
        public_key_url: "https://publickeys.newrelic.com/g/agent-control-oci/global/agent-control-artifacts/jwks.json"
"#,
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
    );
    config::update_config(linux::DEFAULT_AC_CONFIG_PATH, &self_update_config);

    restart_service_and_wait(linux::SERVICE_NAME, STATUS_RUNNING);
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

    let new_version = retry_panic(
        10,
        Duration::from_secs(2),
        "fetching latest AC tag from Docker Hub",
        latest_published_ac_tag,
    );
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
