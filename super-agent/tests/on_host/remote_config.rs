use crate::common::opamp::{ConfigResponse, FakeServer};
use crate::common::retry::retry;
use crate::on_host::tools::instance_id::get_instance_id;
use crate::on_host::tools::super_agent::start_super_agent_with_custom_config;
use newrelic_super_agent::super_agent::config::{AgentID, SuperAgentDynamicConfig};
use newrelic_super_agent::super_agent::defaults::{set_local_dir, set_remote_dir};
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;
use url::Url;

#[cfg(unix)]
#[test]
fn onhost_opamp_superagent_configuration_change() {
    // Given a super-agent without agents and opamp configured.
    let mut server = FakeServer::start_new();
    let server_endpoint = Url::try_from(server.endpoint().as_str()).unwrap();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    set_local_dir(local_dir.path());
    set_remote_dir(remote_dir.path());

    let config_file_path = local_dir.path().join("config.yaml");
    let mut local_file =
        File::create(config_file_path.clone()).expect("failed to create local config file");
    let local_config = r#"
host_id: integration-test
fleet_id: integration
opamp:
  endpoint: http://127.0.0.1/v1/opamp
agents: {}
"#;
    write!(local_file, "{}", local_config).unwrap();

    // We won't join and wait for the thread to finish because we want the super_agent to exit
    // if our assertions were not ok.
    let _super_agent_join = thread::spawn(move || {
        start_super_agent_with_custom_config(config_file_path.as_path(), server_endpoint)
    });

    let super_agent_instance_id = get_instance_id(&AgentID::new_super_agent_id());

    // When a new config with two agents is received from OpAMP
    server.set_config_response(
        super_agent_instance_id,
        ConfigResponse::from(
            r#"
agents:
  nr-infra-agent:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.1.2"
  otel-collector:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
"#,
        ),
    );

    // Then the config should be updated in the remote filesystem.
    let expected_config = r#"agents:
  nr-infra-agent:
    agent_type: newrelic/com.newrelic.infrastructure_agent:0.1.2
  otel-collector:
    agent_type: newrelic/io.opentelemetry.collector:0.0.1
"#;
    let expected_config_parsed =
        serde_yaml::from_str::<SuperAgentDynamicConfig>(expected_config).unwrap();

    retry(20, Duration::from_secs(5), || {
        || -> Result<(), Box<dyn Error>> {
            let remote_file = remote_dir.path().join("config.yaml");
            let content =
                std::fs::read_to_string(remote_file.as_path()).unwrap_or("agents:".to_string());
            let content_parsed =
                serde_yaml::from_str::<SuperAgentDynamicConfig>(content.as_str()).unwrap();
            if content_parsed != expected_config_parsed {
                return Err(format!(
                    "Super agent config not as expected, Expected: {:?}, Found: {:?}",
                    expected_config, content,
                )
                .into());
            }
            Ok(())
        }()
    });

    // TODO: Then OpAMP receives applied (& applying?) AgentToServer (check state on the server).
    // TODO: Then the two agent processes are running (we should create custom agent_types for custom binary).
}
