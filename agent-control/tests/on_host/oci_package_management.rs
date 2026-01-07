use crate::on_host::tools::{
    oci_artifact::push_artifact, oci_package_manager::new_testing_oci_package_manager,
};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::package::manager::PackageManager;
use newrelic_agent_control::package::oci::package_manager::compute_path_suffix;

// Registry created in the make target executing oci-registry.sh
const REGISTRY_URL: &str = "localhost:5001";

#[test]
#[ignore = "needs oci registry, needs elevated privileges"]
fn test_install_and_uninstall_with_oci_registry() {
    const ARTIFACT_CONTENT: &str = "some-content";

    let (_artifact_digest, reference) = push_artifact(ARTIFACT_CONTENT, REGISTRY_URL);

    let temp_dir = tempfile::tempdir().unwrap();
    let base_path = temp_dir.path().to_path_buf();

    let package_manager = new_testing_oci_package_manager(base_path.clone());

    let agent_id = AgentID::try_from("test-agent").unwrap();

    // Install
    let installed_path = package_manager.install(&agent_id, &reference);

    assert!(
        installed_path.is_ok(),
        "Installation failed: {:?}",
        installed_path.unwrap_err()
    );

    let installed_path = installed_path.unwrap();
    assert!(installed_path.exists());
    let content = std::fs::read_to_string(&installed_path).unwrap();
    assert_eq!(content, ARTIFACT_CONTENT);

    // Verify location
    // The path should be base_path/agent_id/oci_registry__port__repo_tag
    let expected_filename = compute_path_suffix(&reference).unwrap();

    let expected_path = base_path
        .join(&agent_id)
        .join("packages")
        .join(expected_filename);

    assert_eq!(installed_path, expected_path);

    // Uninstall
    package_manager
        .uninstall(&agent_id, &installed_path)
        .unwrap();
    assert!(!installed_path.exists());
}
