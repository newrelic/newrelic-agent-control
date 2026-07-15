use std::{fs::read_to_string, path::Path, time::Duration};

use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::base_paths::TempBasePaths;
use crate::common::remote_config_status::check_latest_remote_config_status;
use crate::common::retry::retry;
use crate::common::runtime::tokio_runtime;
use crate::on_host::consts::NO_CONFIG;
use crate::on_host::tools::config::{OnHostAgentControlConfigBuilder, create_local_config};
use crate::on_host::tools::custom_agent_type::OnHostCustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::SHARED_FILESYSTEM_FOLDER_NAME;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use opamp_client::opamp::proto::RemoteConfigStatuses;

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

    let redis_type = ohi_binary_agent_type("redis", "nri-redis").write(dirs.local_dir());
    let mysql_type = ohi_binary_agent_type("mysql", "nri-mysql").write(dirs.local_dir());

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

    let agent_type = OnHostCustomAgentType::default()
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
        .write(dirs.local_dir());

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

/// Uninstalling an OHI (removed from the agents list via an AC remote config) deletes its config
/// file from the shared filesystem, so the infra-agent stops running it. A second OHI sharing the
/// same co-owned directory keeps its own file, and the directory itself is preserved.
#[test]
fn uninstalling_agent_removes_its_shared_file_and_keeps_others() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let dirs = TempBasePaths::default();

    let redis_agent = "redis-agent";
    let mysql_agent = "mysql-agent";
    let redis_type = ohi_config_agent_type("redis", "nri-redis.yaml").write(dirs.local_dir());
    let mysql_type = ohi_config_agent_type("mysql", "nri-mysql.yaml").write(dirs.local_dir());

    // Both agents start from local config.
    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(format!(
            r#"
  {redis_agent}:
    agent_type: "{redis_type}"
  {mysql_agent}:
    agent_type: "{mysql_type}"
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

    let shared_config_dir = dirs
        .remote_dir()
        .join(SHARED_FILESYSTEM_FOLDER_NAME)
        .join(SHARED_CONFIG_DIR);
    let redis_file = shared_config_dir.join("nri-redis.yaml");
    let mysql_file = shared_config_dir.join("nri-mysql.yaml");

    // Both agents write their shared config into the co-owned directory.
    retry(30, Duration::from_secs(1), || {
        expect_file_content(&redis_file, "integration: redis")?;
        expect_file_content(&mysql_file, "integration: mysql")?;
        Ok(())
    });

    // Uninstall redis-agent by pushing an AC remote config that keeps only mysql-agent.
    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());
    opamp_server.set_config_response(
        ac_instance_id,
        format!(
            r#"
agents:
  {mysql_agent}:
    agent_type: "{mysql_type}"
"#
        ),
    );

    // redis's shared file is removed; mysql's file and the co-owned directory remain.
    retry(60, Duration::from_secs(1), || {
        if redis_file.exists() {
            return Err(format!("redis shared file should be removed: {redis_file:?}").into());
        }
        expect_file_content(&mysql_file, "integration: mysql")?;
        if !shared_config_dir.is_dir() {
            return Err("co-owned shared directory should be preserved".into());
        }
        Ok(())
    });
}

/// A file left in the shared filesystem by an agent removed while Agent Control was stopped (so its
/// type is no longer known) is reclaimed by the startup reconcile, while a configured agent's file
/// in the same co-owned directory is preserved.
#[test]
fn startup_reconcile_removes_files_of_agents_removed_while_stopped() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());
    let dirs = TempBasePaths::default();

    let redis_agent = "redis-agent";
    let redis_type = ohi_config_agent_type("redis", "nri-redis.yaml").write(dirs.local_dir());

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

    // Seed a leftover file that belongs to no configured agent, as if written by an agent that was
    // removed from the config while Agent Control was stopped.
    let shared_config_dir = dirs
        .remote_dir()
        .join(SHARED_FILESYSTEM_FOLDER_NAME)
        .join(SHARED_CONFIG_DIR);
    std::fs::create_dir_all(&shared_config_dir).unwrap();
    let departed_file = shared_config_dir.join("nri-departed.yaml");
    std::fs::write(&departed_file, "integration: departed").unwrap();

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    // The configured agent writes its file; the leftover file is reclaimed by the startup reconcile.
    let redis_file = shared_config_dir.join("nri-redis.yaml");
    retry(30, Duration::from_secs(1), || {
        expect_file_content(&redis_file, "integration: redis")?;
        if departed_file.exists() {
            return Err("departed agent's file should be reclaimed at startup".into());
        }
        Ok(())
    });
}

/// A minimal OHI-style agent type that writes a single config file into the shared co-owned
/// `infra-agent-ohi-configs` directory.
fn ohi_config_agent_type(type_name: &str, file: &str) -> OnHostCustomAgentType {
    OnHostCustomAgentType::default()
        .with_agent_type_id(&format!("test/{type_name}:0.1.0"))
        .with_health(None)
        .with_shared_filesystem(Some(&format!(
            r#"
{SHARED_CONFIG_DIR}:
  kind: dir
  entries:
    {file}:
      kind: file
      text: "integration: {type_name}"
"#
        )))
}

/// Two agents of the same OHI type both declare the same shared file. The remote config that
/// introduces the second one must be rejected (single-owner rule) and reported Failed to Fleet
/// Control, rather than letting the two agents fight over the shared path.
#[test]
fn conflicting_shared_paths_are_rejected() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let dirs = TempBasePaths::default();

    let redis_agent = "redis-agent";
    let redis_agent_2 = "redis-agent-2";

    let ohi_type = OnHostCustomAgentType::default()
        .with_agent_type_id("test/redis:0.1.0")
        .with_health(None)
        .with_shared_filesystem(Some(&format!(
            r#"
{SHARED_CONFIG_DIR}:
  kind: dir
  entries:
    nri-redis.yaml:
      kind: file
      text: "integration: redis"
"#
        )))
        .write(dirs.local_dir());

    // Start with a single, valid agent.
    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(format!(
            r#"
  {redis_agent}:
    agent_type: "{ohi_type}"
"#
        ))
        .write(dirs.local_dir());
    create_local_config(
        redis_agent.to_string(),
        NO_CONFIG.to_string(),
        dirs.local_dir(),
    );

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    // Baseline: the single agent writes its shared file.
    let shared_file = dirs
        .remote_dir()
        .join(SHARED_FILESYSTEM_FOLDER_NAME)
        .join(SHARED_CONFIG_DIR)
        .join("nri-redis.yaml");
    retry(30, Duration::from_secs(1), || {
        expect_file_content(&shared_file, "integration: redis")
    });

    // Push an AC remote config adding a second agent of the SAME type: both would claim the same
    // shared file, so the config must be rejected before anything is applied.
    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());
    opamp_server.set_config_response(
        ac_instance_id.clone(),
        format!(
            r#"
agents:
  {redis_agent}:
    agent_type: "{ohi_type}"
  {redis_agent_2}:
    agent_type: "{ohi_type}"
"#
        ),
    );

    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status(&opamp_server, &ac_instance_id, |status| {
            if status.status != RemoteConfigStatuses::Failed as i32 {
                return Err(format!("expected Failed status, got {:?}", status.status).into());
            }
            // The rejection reason surfaced to Fleet Control must point at the conflict: both
            // agents and the shared path they fight over.
            for expected in [
                "shared filesystem conflict",
                redis_agent,
                redis_agent_2,
                "nri-redis.yaml",
            ] {
                if !status.error_message.contains(expected) {
                    return Err(format!(
                        "error message {:?} should mention {expected:?}",
                        status.error_message
                    )
                    .into());
                }
            }
            Ok(())
        })
    });
}

/// A minimal OHI-style agent type that renders a "binary" into its own per-agent filesystem
/// (`bin/<binary>`) and then copies it into the shared binaries dir with `copy_from_file`.
fn ohi_binary_agent_type(type_name: &str, binary: &str) -> OnHostCustomAgentType {
    let payload = binary_payload(binary);
    OnHostCustomAgentType::default()
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
