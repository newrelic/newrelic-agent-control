use super::tools::instance_id::get_instance_id;
use crate::common::opamp::{ConfigResponse, FakeServer};
use crate::common::retry::retry;
use crate::common::super_agent::{init_sa, run_sa};
use newrelic_super_agent::super_agent::config::{AgentID, SuperAgentDynamicConfig};
use newrelic_super_agent::super_agent::defaults::SUPER_AGENT_DATA_DIR;
use std::error::Error;
use std::thread;
use std::time::Duration;
use std::{path::PathBuf, str::FromStr};
use tempfile::TempDir;

#[cfg(unix)]
#[serial_test::serial]
#[test]
fn onhost_opamp_superagent_configuration_change() {
    // Given a super-agent without agents and opamp configured.
    let mut opamp_server = FakeServer::start_new();

    let debug_dir = TempDir::new().unwrap();

    let super_agent_config = format!(
        r#"
host_id: integration-test
opamp:
  endpoint: {}
agents: {{}}    
    "#,
        opamp_server.endpoint()
    );

    let (sa_run_cfg, _guard) = init_sa(debug_dir.path(), &super_agent_config);

    let _super_agent_join = thread::spawn(move || {
        run_sa(sa_run_cfg);
    });

    let super_agent_instance_id = get_instance_id(&AgentID::new_super_agent_id());

    // When a new config with two agents is received from OpAMP
    opamp_server.set_config_response(
        super_agent_instance_id,
        ConfigResponse::from(
            r#"
agents:
  nr-infra-agent:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.1.2"
"#,
        ),
    );

    // Then the config should be updated in the remote filesystem.
    let expected_config = r#"agents:
  nr-infra-agent:
    agent_type: newrelic/com.newrelic.infrastructure_agent:0.1.2
"#;
    let expected_config_parsed =
        serde_yaml::from_str::<SuperAgentDynamicConfig>(expected_config).unwrap();

    retry(15, Duration::from_secs(5), || {
        || -> Result<(), Box<dyn Error>> {
            let remote_file = PathBuf::from_str(&SUPER_AGENT_DATA_DIR())
                .unwrap()
                .join("config.yaml");
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
