use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::base_paths::TempBasePaths;
use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::{retry::retry, runtime::tokio_runtime};
use crate::on_host::tools::config::OnHostAgentControlConfigBuilder;
use crate::on_host::tools::custom_agent_type::OnHostCustomAgentTypeBuilder;
use crate::on_host::tools::instance_id::get_instance_id;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::collections::HashMap;
use std::time::Duration;

#[test]
fn onhost_ac_multiconfig_agents_append() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();

    let sleep_agent_type = OnHostCustomAgentTypeBuilder::default().write(dirs.local_dir());

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

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
        ac_instance_id.clone(),
        HashMap::from([
            ("agentConfig-a".to_string(), agent_a),
            ("agentConfig-b".to_string(), agent_b),
            ("new-feature-coming".to_string(), "oh-yeah".to_string()),
            (
                "override.agentConfig".to_string(),
                "ignored as not supported for AC".to_string(),
            ),
            (
                "variable.agentConfig".to_string(),
                "ignored as not supported for AC".to_string(),
            ),
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
fn onhost_ac_multiconfig_agents_append_fails() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();

    let sleep_agent_type = OnHostCustomAgentTypeBuilder::default().write(dirs.local_dir());

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

    let config = format!(
        r#"
        agents:
          agent-a:
            agent_type: "{sleep_agent_type}"
        "#
    );

    opamp_server.set_multi_config_response(
        ac_instance_id.clone(),
        HashMap::from([
            // Both configs define agent-a, causing a conflict.
            ("agentConfig-a".to_string(), config.clone()),
            ("agentConfig-b".to_string(), config),
        ]),
    );

    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Failed as i32,
        )?;

        Ok(())
    });
}

#[test]
fn onhost_sub_agent_multiconfig() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();

    let sleep_agent_type = OnHostCustomAgentTypeBuilder::default()
        .with_variables(
            r#"
var_a:
  description: "a"
  type: "string"
  required: true
var_b:
  description: "b"
  type: "string"
  required: true
var_c:
  description: "c"
  type: "string"
  required: true
var_d:
  description: "d"
  type: "string"
  required: true
var_e:
  description: "map of file names to their contents"
  type: string_map
  required: false
  default: { }

    "#,
        )
        .write(dirs.local_dir());

    let agent_id = "nr-sleep-agent";
    let agents = format!(
        r#"
  {agent_id}:
    agent_type: "{sleep_agent_type}"
"#
    );

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents)
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let sub_agent_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), dirs.base_paths());

    let expected_config = r#"
var_a: a
var_b: b
var_c: overridden_c
var_d: overridden_var_d
var_e:
  file1.yaml:
    content: new
  file2.yaml:
    content: added
  file3.yaml:
    content: keep
"#;

    opamp_server.set_multi_config_response(
        sub_agent_instance_id.clone(),
        HashMap::from([
            ("agentConfig".to_string(), "var_a: a".to_string()),
            ("agentConfig-b".to_string(), "var_b: b".to_string()),
            ("agentConfig-c".to_string(), "var_c: c".to_string()),
            (
                "override.agentConfig".to_string(),
                "var_c: overridden_c".to_string(),
            ),
            (
                "variable.agentConfig.var_d".to_string(),
                "overridden_var_d".to_string(),
            ),
            (
                "variable.agentConfig.var_e".to_string(),
                "file1.yaml:\n  content: old\nfile3.yaml:\n  content: keep".to_string(),
            ),
            (
                "variable.agentConfig.var_e:file1.yaml".to_string(),
                "content: new".to_string(),
            ),
            (
                "variable.agentConfig.var_e:file2.yaml".to_string(),
                "content: added".to_string(),
            ),
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
