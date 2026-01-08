use crate::on_host::tools::{
    oci_artifact::push_artifact, oci_package_manager::new_testing_oci_package_manager,
};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::package::manager::{PackageData, PackageManager};
use newrelic_agent_control::package::oci::package_manager::compute_path_suffix;

// Registry created in the make target executing oci-registry.sh
const REGISTRY_URL: &str = "localhost:5001";

#[test]
#[ignore = "needs oci registry, needs elevated privileges on Windows"]
fn test_install_and_uninstall_with_oci_registry() {
    const ARTIFACT_CONTENT: &str = "some-content";

    let (_artifact_digest, reference) = push_artifact(ARTIFACT_CONTENT, REGISTRY_URL);

    let temp_dir = tempfile::tempdir().unwrap();
    let base_path = temp_dir.path().to_path_buf();

    let package_manager = new_testing_oci_package_manager(base_path.clone());

    let agent_id = AgentID::try_from("test-agent").unwrap();
    let pkg_id = "test-package";

    // Install
    let package_data = PackageData {
        id: pkg_id.to_string(),
        oci_reference: reference.clone(),
    };
    let installed_package_result = package_manager.install(&agent_id, package_data);

    assert!(
        installed_package_result.is_ok(),
        "Installation failed: {:?}",
        installed_package_result.as_ref().unwrap_err()
    );

    let installed_package = installed_package_result.unwrap();
    assert!(installed_package.installation_path.exists());
    let content = std::fs::read_to_string(&installed_package.installation_path).unwrap();
    assert_eq!(content, ARTIFACT_CONTENT);

    // Verify location
    // The path should be base_path/agent_id/oci_registry__port__repo_tag
    let expected_filename = compute_path_suffix(&reference).unwrap();

    let expected_path = base_path
        .join(&agent_id)
        .join("packages")
        .join(pkg_id)
        .join(expected_filename);

    assert_eq!(installed_package.installation_path, expected_path);

    // Uninstall
    let installation_path = installed_package.installation_path.clone();
    package_manager
        .uninstall(&agent_id, installed_package)
        .unwrap();
    assert!(!installation_path.exists());
}
