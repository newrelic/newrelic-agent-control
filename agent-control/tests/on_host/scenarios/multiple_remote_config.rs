use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::{opamp::FakeServer, retry::retry};
use crate::on_host::tools::config::create_agent_control_config;
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::opamp::remote_config::AGENT_CONFIG_PREFIX;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::collections::HashMap;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn onhost_ac_multiconfig_agents_append() {
    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let sleep_agent_type = CustomAgentType::default().build(local_dir.path().to_path_buf());

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        "{}".to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, base_paths.clone());

    let agent_a = format!(
        r#"
        agents:
          agent-a:
            agent_type: "{sleep_agent_type}"
        "#
    );

    let agent_b = format!(
        r#"
        agents:
          agent-b:
            agent_type: "{sleep_agent_type}"
            "#
    );

    let expected_config = format!(
        r#"
        agents:
          agent-a:
            agent_type: "{sleep_agent_type}"
          agent-b:
            agent_type: "{sleep_agent_type}"
            "#
    );

    opamp_server.set_multi_config_response(
        &ac_instance_id,
        HashMap::from([
            (format!("{AGENT_CONFIG_PREFIX}-a"), agent_a),
            (format!("{AGENT_CONFIG_PREFIX}-b"), agent_b),
            ("new-feature-coming".to_string(), "oh-yeah".to_string()),
        ]),
    );

    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32,
        )?;
        check_latest_effective_config_is_expected(
            &opamp_server,
            &ac_instance_id,
            expected_config.clone(),
        )?;
        Ok(())
    });
}

#[test]
fn onhost_sub_agent_multiconfig() {
    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let sleep_agent_type = CustomAgentType::default()
        .with_variables(
            r#"
    var_a:
      description: "foo"
      type: "string"
      required: true
    var_b:
      description: "bar"
      type: "string"
      required: true
    "#,
        )
        .build(local_dir.path().to_path_buf());

    let agent_id = "nr-sleep-agent";
    let agents = format!(
        r#"
  {agent_id}:
    agent_type: "{sleep_agent_type}"
"#
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        agents,
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    let sub_agent_instance_id = get_instance_id(&AgentID::try_from(agent_id).unwrap(), base_paths);

    let var_a = "var_a: a".to_string();
    let var_b = "var_b: b".to_string();
    let expected_config = "var_a: a\nvar_b: b";

    opamp_server.set_multi_config_response(
        &sub_agent_instance_id,
        HashMap::from([
            (format!("{AGENT_CONFIG_PREFIX}-a"), var_a),
            (format!("{AGENT_CONFIG_PREFIX}-b"), var_b),
            ("new-feature-coming".to_string(), "oh-yeah".to_string()),
        ]),
    );

    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            RemoteConfigStatuses::Applied as i32,
        )?;
        check_latest_effective_config_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            expected_config.to_string(),
        )?;
        Ok(())
    });
}
