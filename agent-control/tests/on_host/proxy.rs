use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::health::check_latest_health_status_was_healthy;
use crate::common::{opamp::FakeServer, retry::retry};
use crate::on_host::tools::config::create_agent_control_config_with_proxy;
use crate::on_host::tools::instance_id::get_instance_id;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::BasePaths;
use std::time::Duration;
use tempfile::tempdir;

/// Check proxy configuration in a simple scenario involving OpAMP
/// In order to execute the integration test a proxy needs to be up and running. Example:
///
/// ```no-test
/// # Run a simple proxy
/// $ docker run --rm --name mitmproxy -v /tmp/mitmproxy:/home/mitmproxy/.mitmproxy -p 8080:8080 mitmproxy/mitmproxy mitmdump
/// # Execute the corresponding test
/// $ TESTING_PROXY_URL="http://localhost:8080" TESTING_PROXY_CA_DIR=/tmp/mitmproxy TESTING_HOST_GATEWAY="host.docker.internal" cargo test --test integration_tests -- proxy_ --ignored
/// ```
// TODO: run this on CI
#[cfg(unix)]
#[ignore = "needs proxy up and running"]
#[test]
fn proxy_onhost_opamp_agent_control_local_effective_config() {
    // Given a agent-control without agents and opamp configured.

    use std::env;

    use newrelic_agent_control::agent_control::run::Environment;

    use crate::common::agent_control::start_agent_control_with_custom_config;
    let opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");
    // Setup proxy env variables
    let proxy_url =
        env::var("TESTING_PROXY_URL").expect("Required TESTING_PROXY_URL env var not defined");
    let proxy_ca_dir = env::var("TESTING_PROXY_CA_DIR")
        .expect("Required TESTING_PROXY_CA_DIR env var not defined");
    let host_gateway = env::var("TESTING_HOST_GATEWAY")
        .expect("Required TESTING_HOST_GATEWAY env var not defined");
    // Needed to reach the host's localhost from a container
    let opamp_server_endpoint = opamp_server
        .endpoint()
        .replace("localhost", host_gateway.as_str());

    let agents = "{}";

    create_agent_control_config_with_proxy(
        opamp_server_endpoint,
        agents.to_string(),
        local_dir.path().to_path_buf(),
        Some(format!(
            "{{\"url\": \"{proxy_url}\", \"ca_bundle_dir\": \"{proxy_ca_dir}\"}}"
        )),
        opamp_server.cert_file_path(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let base_paths = base_paths.clone();

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);

    let agent_control_instance_id = get_instance_id(&AgentID::new_agent_control_id(), base_paths);

    retry(60, Duration::from_secs(1), || {
        let expected_config = "agents: {}\n";

        check_latest_effective_config_is_expected(
            &opamp_server,
            &agent_control_instance_id,
            expected_config.to_string(),
        )?;
        check_latest_health_status_was_healthy(&opamp_server, &agent_control_instance_id)
    });
}
