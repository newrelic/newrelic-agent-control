use crate::common::{
    agent_control::start_agent_control_with_custom_config, base_paths::TempBasePaths,
    effective_config::check_latest_effective_config_is_expected, retry::retry,
    runtime::tokio_runtime,
};
use crate::on_host::tools::{
    config::OnHostAgentControlConfigBuilder, custom_agent_type::OnHostCustomAgentTypeBuilder,
    instance_id::get_instance_id,
};
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::{
    agent_id::AgentID, run::on_host::AGENT_CONTROL_MODE_ON_HOST,
};
use std::time::Duration;

/// When a sub-agent's type is upgraded to a new version (same namespace + name, different
/// version), Agent Control must preserve the InstanceID. Fleet Control keeps continuity with
/// the already-connected agent across the upgrade.
#[test]
fn version_bump_preserves_instance_id() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let dirs = TempBasePaths::default();
    let agent_id = "my-agent";

    // Register both versions so the registry can resolve them during filesystem reconciliation.
    let v1 = OnHostCustomAgentTypeBuilder::empty()
        .with_agent_type_id("newrelic/com.newrelic.custom_agent:0.1.0")
        .write(dirs.local_dir());
    let v2 = OnHostCustomAgentTypeBuilder::empty()
        .with_agent_type_id("newrelic/com.newrelic.custom_agent:0.2.0")
        .write(dirs.local_dir());

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .write(dirs.local_dir());
    let _ac = start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());
    let agent_id_typed = AgentID::try_from(agent_id).unwrap();

    // Deploy v1 and wait for AC to report it in its effective config.
    opamp_server.set_config_response(
        ac_id.clone(),
        format!("agents:\n  {agent_id}:\n    agent_type: \"{v1}\""),
    );
    retry(60, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &ac_id,
            format!("agents:\n  {agent_id}:\n    agent_type: \"{v1}\""),
        )
    });

    let id_before = get_instance_id(&agent_id_typed, dirs.base_paths());

    // Bump to v2 — same type name, triggers the version-bump path.
    opamp_server.set_config_response(
        ac_id.clone(),
        format!("agents:\n  {agent_id}:\n    agent_type: \"{v2}\""),
    );
    retry(60, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &ac_id,
            format!("agents:\n  {agent_id}:\n    agent_type: \"{v2}\""),
        )
    });

    assert_eq!(
        id_before,
        get_instance_id(&agent_id_typed, dirs.base_paths()),
        "InstanceID must not change after a version bump"
    );
}

/// When a sub-agent's type is replaced with a fully different type (namespace or name changes),
/// Agent Control must reset the InstanceID so the replacement agent starts fresh.
#[test]
fn type_change_resets_instance_id() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let dirs = TempBasePaths::default();
    let agent_id = "my-agent";

    // Two types with different names — triggers the full-replacement path.
    let type_a = OnHostCustomAgentTypeBuilder::empty()
        .with_agent_type_id("newrelic/com.newrelic.custom_agent_a:0.1.0")
        .write(dirs.local_dir());
    let type_b = OnHostCustomAgentTypeBuilder::empty()
        .with_agent_type_id("newrelic/com.newrelic.custom_agent_b:0.1.0")
        .write(dirs.local_dir());

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .write(dirs.local_dir());
    let _ac = start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());
    let agent_id_typed = AgentID::try_from(agent_id).unwrap();

    // Deploy type A and wait for AC to confirm.
    opamp_server.set_config_response(
        ac_id.clone(),
        format!("agents:\n  {agent_id}:\n    agent_type: \"{type_a}\""),
    );
    retry(60, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &ac_id,
            format!("agents:\n  {agent_id}:\n    agent_type: \"{type_a}\""),
        )
    });

    let id_before = get_instance_id(&agent_id_typed, dirs.base_paths());

    // Switch to type B — different name, triggers the full-replacement path.
    opamp_server.set_config_response(
        ac_id.clone(),
        format!("agents:\n  {agent_id}:\n    agent_type: \"{type_b}\""),
    );
    retry(60, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &ac_id,
            format!("agents:\n  {agent_id}:\n    agent_type: \"{type_b}\""),
        )
    });

    assert_ne!(
        id_before,
        get_instance_id(&agent_id_typed, dirs.base_paths()),
        "InstanceID must change after a full type swap"
    );
}
