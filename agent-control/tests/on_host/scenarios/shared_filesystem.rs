use std::{fs::read_to_string, path::Path, time::Duration};

use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::base_paths::TempBasePaths;
use crate::common::retry::retry;
use crate::common::runtime::tokio_runtime;
use crate::on_host::consts::NO_CONFIG;
use crate::on_host::tools::config::{OnHostAgentControlConfigBuilder, create_local_config};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::SHARED_FILESYSTEM_FOLDER_NAME;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;

const SHARED_CONFIG_DIR: &str = "infra-agent-ohi-configs";

const SHARED_BIN_DIR: &str = "infra-agent-ohi-binaries";

/// Two OHI agent types each render a "binary" into their own per-agent filesystem, then copy it
/// into the shared filesystem via `copy_from_file`. This covers the two paths that drive a shared
/// write: `redis-agent` is present at AC startup (local config), while `mysql-agent` is added at
/// runtime through an AC-level OpAMP remote config. Both entries coexist under the shared base.
#[test]
fn multiple_agents_write_to_shared_filesystem() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let dirs = TempBasePaths::default();

    let redis_agent = "redis-agent";
    let mysql_agent = "mysql-agent";

    let redis_type = ohi_binary_agent_type("redis", "nri-redis").build(dirs.local_dir());
    let mysql_type = ohi_binary_agent_type("mysql", "nri-mysql").build(dirs.local_dir());

    // Only redis-agent is in the local config, so it starts at AC startup. mysql-agent is added
    // later via remote config; its values file is created up front so it can be assembled then.
    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(format!(
            r#"
  {redis_agent}:
    agent_type: "{redis_type}"
"#
        ))
        .write(dirs.local_dir());
    create_local_config(
        redis_agent.to_string(),
        NO_CONFIG.to_string(),
        dirs.local_dir(),
    );
    create_local_config(
        mysql_agent.to_string(),
        NO_CONFIG.to_string(),
        dirs.local_dir(),
    );

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let shared_bin_dir = dirs
        .remote_dir()
        .join(SHARED_FILESYSTEM_FOLDER_NAME)
        .join(SHARED_BIN_DIR);
    let redis_file = shared_bin_dir.join("nri-redis");
    let mysql_file = shared_bin_dir.join("nri-mysql");

    // AC startup path: redis-agent writes its shared binary.
    retry(30, Duration::from_secs(1), || {
        expect_file_content(&redis_file, &binary_payload("nri-redis"))
    });

    // Add-a-new-agent path: push an AC remote config that also includes mysql-agent.
    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());
    opamp_server.set_config_response(
        ac_instance_id,
        format!(
            r#"
agents:
  {redis_agent}:
    agent_type: "{redis_type}"
  {mysql_agent}:
    agent_type: "{mysql_type}"
"#
        ),
    );

    // mysql-agent is now started and writes its shared binary; redis-agent's file remains.
    retry(60, Duration::from_secs(1), || {
        expect_file_content(&mysql_file, &binary_payload("nri-mysql"))?;
        expect_file_content(&redis_file, &binary_payload("nri-redis"))?;
        Ok(())
    });
}

/// A config update pushed via OpAMP remote config re-renders and rewrites the shared filesystem
/// entry: the sub-agent re-applies and the file on disk reflects the new value.
#[test]
fn shared_filesystem_entry_updated_via_opamp_remote_config() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let dirs = TempBasePaths::default();

    let agent_id = "ohi-agent";

    let agent_type = CustomAgentType::default()
        .with_health(None)
        .with_variables(
            r#"
ohi_config:
  description: "OHI config file content"
  type: "string"
  required: false
  default: "v1"
"#,
        )
        .with_shared_filesystem(Some(&format!(
            r#"
{SHARED_CONFIG_DIR}:
  kind: dir
  entries:
    nri-redis.yaml:
      kind: file
      text: ${{nr-var:ohi_config}}
"#
        )))
        .build(dirs.local_dir());

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(format!(
            r#"
  {agent_id}:
    agent_type: "{agent_type}"
"#
        ))
        .write(dirs.local_dir());
    create_local_config(
        agent_id.to_string(),
        NO_CONFIG.to_string(),
        dirs.local_dir(),
    );

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let shared_file = dirs
        .remote_dir()
        .join(SHARED_FILESYSTEM_FOLDER_NAME)
        .join(SHARED_CONFIG_DIR)
        .join("nri-redis.yaml");

    // Install: the entry is written with the variable's default value.
    retry(30, Duration::from_secs(1), || {
        expect_file_content(&shared_file, "v1")
    });

    // Update: push new values over OpAMP; the sub-agent re-applies and rewrites the entry.
    let sub_instance_id = get_instance_id(&AgentID::try_from(agent_id).unwrap(), dirs.base_paths());
    opamp_server.set_config_response(sub_instance_id, "ohi_config: v2");

    retry(60, Duration::from_secs(1), || {
        expect_file_content(&shared_file, "v2")
    });
}

/// A minimal OHI-style agent type that renders a "binary" into its own per-agent filesystem
/// (`bin/<binary>`) and then copies it into the shared binaries dir with `copy_from_file`.
fn ohi_binary_agent_type(type_name: &str, binary: &str) -> CustomAgentType {
    let payload = binary_payload(binary);
    CustomAgentType::default()
        .with_agent_type_id(&format!("test/{type_name}:0.1.0"))
        .with_health(None)
        .with_filesystem(Some(&format!(
            r#"
bin:
  kind: dir
  entries:
    {binary}:
      kind: file
      text: "{payload}"
"#
        )))
        .with_shared_filesystem(Some(&format!(
            r#"
{SHARED_BIN_DIR}:
  kind: dir
  entries:
    {binary}:
      kind: file
      copy_from_file: ${{nr-sub:filesystem_agent_dir}}/bin/{binary}
"#
        )))
}

fn binary_payload(binary: &str) -> String {
    format!("{binary}-payload")
}

fn expect_file_content(path: &Path, expected: &str) -> Result<(), Box<dyn std::error::Error>> {
    match read_to_string(path) {
        Ok(contents) if contents == expected => Ok(()),
        Ok(contents) => Err(format!("unexpected content at {path:?}: {contents:?}").into()),
        Err(err) => Err(format!("file not present yet at {path:?}: {err}").into()),
    }
}
