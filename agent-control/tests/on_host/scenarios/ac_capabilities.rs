use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::attributes::check_custom_capabilities_match;
use crate::common::base_paths::TempBasePaths;
use crate::common::retry::retry;
use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::config::OnHostAgentControlConfigBuilder;
use crate::on_host::tools::instance_id::get_instance_id;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::opamp::capabilities::{CustomCapabilities, CustomCapability};
use std::time::Duration;

#[test]
fn ac_custom_capabilities_all() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

    retry(60, Duration::from_secs(1), || {
        check_custom_capabilities_match(
            &opamp_server,
            &ac_instance_id,
            CustomCapabilities::from(vec![
                CustomCapability::Signature,
                // reaching default docker agent type repo
                CustomCapability::RemoteAgentTypeRepoReachable,
            ]),
        )
        .map_err(|err| err.into())
    });
}

#[test]
fn ac_custom_capabilities_with_agent_type_unreachable() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agent_types(false, "newrelic/missing-repo", "http://newrelic.com")
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

    retry(60, Duration::from_secs(1), || {
        check_custom_capabilities_match(
            &opamp_server,
            &ac_instance_id,
            CustomCapabilities::from(vec![
                CustomCapability::Signature,
                // CustomCapability::RemoteAgentTypeRepoReachable is not reported
            ]),
        )
        .map_err(|err| err.into())
    });
}
