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
use newrelic_agent_control::agent_control::defaults::AGENT_FILESYSTEM_FOLDER_NAME;
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

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agent(agent_id, sleep_agent_type)
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

/// Covers `variable.agentConfig.<var>` with nested variables, ignored extra items in the config-map and
/// "double expansion"
#[test]
fn onhost_sub_agent_multiconfig_only_variable_overrides() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();

    let testing_agent_type = OnHostCustomAgentTypeBuilder::default()
        .with_variables(
            r#"
a:
  really:
    nested:
      config:
        description: "Some nested config"
        type: "string"
        required: true
    "#,
        )
        .with_filesystem(Some(
            r#"
rendered:
  kind: dir
  entries:
    config.txt:
      kind: file
      text: "${nr-var:a.really.nested.config}"
"#,
        ))
        .write(dirs.local_dir());

    let agent_id = "nr-testing-agent-type";

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agent(agent_id, testing_agent_type)
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let sub_agent_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), dirs.base_paths());

    // The override value itself references the sub-agent's filesystem dir via `${nr-path:...}`.
    // This placeholder is expanded before the value is used to fill the agent type's variable
    // tree, so it should stay literal in the effective config but be expanded in the rendered
    // deployment.
    let override_value = "some value with ${nr-path:agent_dir}".to_string();

    let expected_config = format!(
        r#"
a:
  really:
    nested:
      config: "{override_value}"
"#
    );

    opamp_server.set_multi_config_response(
        sub_agent_instance_id.clone(),
        HashMap::from([
            (
                "variable.agentConfig.a.really.nested.config".to_string(),
                override_value,
            ),
            // Not a recognized prefix (agentConfig / override.agentConfig / variable.agentConfig):
            // ignored entirely, does not affect the effective config or the applied status.
            ("unknown-key".to_string(), "ignored".to_string()),
            // A per-variable override for a variable not declared in the agent type: ignored with
            // a warning, does not affect the effective config or the applied status.
            (
                "variable.agentConfig.not.declared".to_string(),
                "ignored".to_string(),
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

    // The overridden value should also expand correctly wherever the agent type's deployment
    // section references it via `${nr-var:a.really.nested.config}`, with the `${nr-path:...}`
    // placeholder nested inside it resolved to the sub-agent's actual filesystem dir.
    let agent_dir = dirs
        .remote_dir()
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id);
    let expected_rendered_content = format!("some value with {}", agent_dir.to_string_lossy());

    let rendered_config_path = dirs
        .remote_dir()
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id)
        .join("rendered")
        .join("config.txt");

    retry(60, Duration::from_secs(1), || {
        let content = std::fs::read_to_string(&rendered_config_path)?;
        if content != expected_rendered_content {
            return Err(format!(
                "Content mismatch: expected '{expected_rendered_content}', got '{content}'"
            )
            .into());
        }
        Ok(())
    });
}

/// Covers switching from a plain `agentConfig` to a config made entirely of `variable.agentConfig.<var>[:<file>]`
/// overrides.
#[test]
fn onhost_sub_agent_multiconfig_agent_config_then_full_variable_override() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();

    let testing_agent_type = OnHostCustomAgentTypeBuilder::default()
        .with_variables(
            r#"
config_agent:
  description: "agent yaml configuration"
  type: yaml
  required: true
files:
  description: "map of file names to their contents"
  type: string_map
  required: true
    "#,
        )
        .write(dirs.local_dir());

    let agent_id = "nr-testing-agent-type";

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agent(agent_id, testing_agent_type)
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let sub_agent_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), dirs.base_paths());

    let initial_agent_config = r#"
config_agent:
  key1: value1
files:
  file1.txt: content1
"#;

    let expected_initial_config = r#"
config_agent:
  key1: value1
files:
  file1.txt: content1
"#;

    opamp_server.set_multi_config_response(
        sub_agent_instance_id.clone(),
        HashMap::from([("agentConfig".to_string(), initial_agent_config.to_string())]),
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
            expected_initial_config.to_string(),
        )?;
        Ok(())
    });

    // Replace the whole config with per-variable overrides only: no `agentConfig` key at all.
    let expected_overridden_config = r#"
config_agent:
  key2: value2
  key3: value3
files:
  file2.txt: content2
  file3.txt: content3
"#;

    opamp_server.set_multi_config_response(
        sub_agent_instance_id.clone(),
        HashMap::from([
            (
                "variable.agentConfig.config_agent".to_string(),
                "key2: value2\nkey3: value3".to_string(),
            ),
            (
                "variable.agentConfig.files:file2.txt".to_string(),
                "content2".to_string(),
            ),
            (
                "variable.agentConfig.files:file3.txt".to_string(),
                "content3".to_string(),
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
            expected_overridden_config.to_string(),
        )?;
        Ok(())
    });
}
