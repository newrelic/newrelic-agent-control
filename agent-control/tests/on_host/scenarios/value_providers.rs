use std::{fs::read_to_string, time::Duration};

use crate::common::base_paths::TempBasePaths;
use crate::on_host::consts::NO_CONFIG;
use crate::{
    common::{
        agent_control::start_agent_control_with_custom_config, retry::retry, runtime::tokio_runtime,
    },
    on_host::tools::{
        config::{OnHostAgentControlConfigBuilder, create_file, create_local_config},
        custom_agent_type::DYNAMIC_AGENT_TYPE_FILENAME,
    },
};
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::defaults::AGENT_FILESYSTEM_FOLDER_NAME;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;

/// A `${nr-file:<path>}` reference in an agent type should be resolved with the contents of the
/// referenced file through the file value provider.
#[test]
fn onhost_template_value_providers() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();

    let agent_id = "test-agent";
    let dir_entry = "example-filepath";
    let file_name = "rendered.txt";

    let source_file = dirs.local_dir().join("source.txt");
    create_file("secret-value\n".to_string(), source_file.clone());

    create_file(
        format!(
            r#"
namespace: test
name: test
version: 0.0.0
platform: host
operating_system: {AGENT_CONTROL_MODE_ON_HOST}
protocol_version: "1.0"
variables: {{}}
deployment:
  filesystem:
    {dir_entry}:
      kind: dir
      entries:
        {file_name}:
          kind: file
          text: "${{nr-file:{source}}}"
"#,
            // Forward slashes keep Windows paths from being read as YAML escape sequences.
            source = source_file.display().to_string().replace('\\', "/")
        ),
        dirs.local_dir().join(DYNAMIC_AGENT_TYPE_FILENAME),
    );

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agent(agent_id, "test/test:0.0.0")
        .write(dirs.local_dir());
    create_local_config(
        agent_id.to_string(),
        NO_CONFIG.to_string(),
        dirs.local_dir(),
    );

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let rendered_path = dirs
        .remote_dir()
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id)
        .join(dir_entry)
        .join(file_name);

    // The file provider trims surrounding whitespace from the file contents.
    retry(30, Duration::from_secs(1), || {
        let content = read_to_string(&rendered_path)
            .map_err(|e| format!("failed to read {}: {e}", rendered_path.display()))?;
        if content != "secret-value" {
            return Err(format!("unexpected file content: {content:?}").into());
        }
        Ok(())
    });
}
