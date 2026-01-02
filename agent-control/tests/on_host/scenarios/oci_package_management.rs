use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::opamp::FakeServer;
use crate::common::retry::retry;
use crate::on_host::tools::config::create_agent_control_config;
use crate::on_host::tools::oci_artifact::push_artifact;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::GENERATED_FOLDER_NAME;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use std::time::Duration;
use tempfile::tempdir;

// Registry URL to be used. Since we can't easily spin up a registry in tests without docker/external scripts,
// this test expects a registry running at localhost:5000 (default for registry:2 image).
// If not available, we should probably ignore the test or make it conditional.
const REGISTRY_URL: &str = "localhost:5000";
const INSTALLED_PACKAGES_LOCATION: &str = "packages";

#[test]
#[ignore = "needs oci registry running at localhost:5000"]
fn test_install_and_uninstall_with_oci_registry() {
    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    // 1. Push an artifact to the registry
    let artifact_content = "some-content";
    let (_digest, reference) = push_artifact(artifact_content, REGISTRY_URL);

    // 2. Configure Agent Control with an agent using this OCI artifact
    let agent_id = "test-oci-agent";
    let package_reference = reference.to_string(); // e.g. localhost:5000/test:timestamp

    // Define an Agent Type that uses this package
    let agent_type_def = format!(
        r#"
namespace: test
name: oci-agent
version: 0.0.1
package:
  oci:
    image: "{}"
deployment:
  linux:
    executables: []
  windows:
    executables: []
"#,
        package_reference
    );

    // Save this agent type locally
    let agent_type_file = local_dir.path().join("dynamic-agent-types/type.yaml");
    std::fs::create_dir_all(agent_type_file.parent().unwrap()).unwrap();
    std::fs::write(&agent_type_file, agent_type_def).unwrap();

    let agent_type_id = "test/oci-agent:0.0.1"; // Matches namespace/name:version

    let agents = format!(
        r#"
  {agent_id}:
    agent_type: "{agent_type_id}"
"#
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
    );

    crate::on_host::tools::config::create_local_config(
        agent_id.to_string(),
        "".to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    // 3. Verify installation
    // The file should be at: remote_dir/generated/agent_id/packages/repo_tag
    let repo = reference.repository();
    let tag = reference.tag().unwrap_or("latest");
    let installed_filename = format!("{}_{}", repo, tag).replace("/", "_");

    let installed_path = base_paths
        .remote_dir
        .join(GENERATED_FOLDER_NAME)
        .join(agent_id)
        .join(INSTALLED_PACKAGES_LOCATION)
        .join(installed_filename);

    retry(60, Duration::from_secs(1), || {
        if installed_path.exists() {
            // Verify content
            let content = std::fs::read_to_string(&installed_path).unwrap();
            if content == artifact_content {
                Ok(())
            } else {
                Err(format!(
                    "Content mismatch: expected {}, got {}",
                    artifact_content, content
                )
                .into())
            }
        } else {
            Err("File not installed yet".into())
        }
    });

    // 4. Uninstall
    // Remove agent using OpAMP remote config
    let remote_config = "agents: {}"; // Empty agents list
    let agent_control_instance_id = crate::on_host::tools::instance_id::get_instance_id(
        &AgentID::AgentControl,
        base_paths.clone(),
    );

    opamp_server.set_config_response(agent_control_instance_id.clone(), remote_config);

    // 5. Verify uninstallation
    retry(60, Duration::from_secs(1), || {
        if !installed_path.exists() {
            Ok(())
        } else {
            Err("File still exists".into())
        }
    });
}
