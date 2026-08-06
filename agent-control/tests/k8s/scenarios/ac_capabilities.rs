use crate::common::attributes::{
    check_capabilities_match_expected, check_custom_capabilities_match,
};
use crate::common::retry::retry;
use crate::common::runtime::{block_on, tokio_runtime};
use crate::k8s::tools::agent_control::start_agent_control;
use crate::k8s::tools::config::K8sAgentControlConfigBuilder;
use crate::k8s::tools::custom_agent_type::K8sCustomAgentTypeBuilder;
use crate::k8s::tools::{instance_id, k8s_env::K8sEnv};
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::default_capabilities;
use newrelic_agent_control::opamp::capabilities::{CustomCapabilities, CustomCapability};
use std::time::Duration;
use tempfile::tempdir;

#[test]
#[ignore = "needs a k8s cluster"]
fn k8s_custom_capabilities_all() {
    let server = FakeServer::start(tokio_runtime().handle());

    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    K8sAgentControlConfigBuilder::new(&namespace)
        .with_fleet(server.endpoint(), server.jwks_endpoint())
        .write(k8s.client.clone(), tmp_dir.path());

    K8sCustomAgentTypeBuilder::default().write(tmp_dir.path());
    let _ac = start_agent_control(k8s.client.clone(), &namespace, tmp_dir.path());

    let instance_id =
        instance_id::get_instance_id(k8s.client.clone(), &namespace, &AgentID::AgentControl);

    retry(60, Duration::from_secs(5), || {
        check_capabilities_match_expected(&server, &instance_id, default_capabilities().into())?;
        check_custom_capabilities_match(
            &server,
            &instance_id,
            CustomCapabilities::from(vec![
                CustomCapability::Signature,
                // reaching default docker agent type repo
                CustomCapability::RemoteAgentTypeRepoReachable,
            ]),
        )?;
        Ok(())
    })
}

#[test]
#[ignore = "needs a k8s cluster"]
fn k8s_custom_capabilities_with_agent_type_unreachable() {
    let server = FakeServer::start(tokio_runtime().handle());

    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    K8sAgentControlConfigBuilder::new(&namespace)
        .with_fleet(server.endpoint(), server.jwks_endpoint())
        .with_agent_types(false, "newrelic/missing-repo", "http://newrelic.com")
        .write(k8s.client.clone(), tmp_dir.path());

    K8sCustomAgentTypeBuilder::default().write(tmp_dir.path());
    let _ac = start_agent_control(k8s.client.clone(), &namespace, tmp_dir.path());

    let instance_id =
        instance_id::get_instance_id(k8s.client.clone(), &namespace, &AgentID::AgentControl);

    retry(60, Duration::from_secs(5), || {
        check_capabilities_match_expected(&server, &instance_id, default_capabilities().into())?;
        check_custom_capabilities_match(
            &server,
            &instance_id,
            CustomCapabilities::from(vec![
                CustomCapability::Signature,
                // CustomCapability::RemoteAgentTypeRepoReachable is not reported
            ]),
        )?;
        Ok(())
    })
}

/// When `cd_enabled` is set to `false` in the k8s config, Agent Control must report the
/// `com.newrelic.k8s_config_only_agents` custom capability through OpAMP. This signals to
/// Fleet Control that this AC is not paired with an agent-control-cd deployment.
#[test]
#[ignore = "needs a k8s cluster"]
fn k8s_test_custom_capabilities_when_cd_disabled() {
    let server = FakeServer::start(tokio_runtime().handle());

    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    K8sAgentControlConfigBuilder::new(&namespace)
        .with_fleet(server.endpoint(), server.jwks_endpoint())
        .with_cd_enabled(false)
        .write(k8s.client.clone(), tmp_dir.path());

    K8sCustomAgentTypeBuilder::default().write(tmp_dir.path());
    let _ac = start_agent_control(k8s.client.clone(), &namespace, tmp_dir.path());

    let instance_id =
        instance_id::get_instance_id(k8s.client.clone(), &namespace, &AgentID::AgentControl);

    retry(60, Duration::from_secs(5), || {
        check_capabilities_match_expected(&server, &instance_id, default_capabilities().into())?;
        check_custom_capabilities_match(
            &server,
            &instance_id,
            CustomCapabilities::from(vec![
                CustomCapability::Signature,
                CustomCapability::K8sConfigOnlyAgents,
                CustomCapability::RemoteAgentTypeRepoReachable,
            ]),
        )?;
        Ok(())
    })
}
