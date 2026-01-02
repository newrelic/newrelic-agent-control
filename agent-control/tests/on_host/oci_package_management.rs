use crate::on_host::tools::oci_artifact::push_artifact;
use fs::{LocalFile, directory_manager::DirectoryManagerFs};
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::http::config::ProxyConfig;
use newrelic_agent_control::package::manager::PackageManager;
use newrelic_agent_control::package::oci::downloader::OCIRefDownloader;
use newrelic_agent_control::package::oci::package_manager::OCIPackageManager;
use oci_client::client::{ClientConfig, ClientProtocol};
use std::sync::Arc;

// Registry created in the make target executing oci-registry.sh
const REGISTRY_URL: &str = "localhost:5000";

#[test]
#[ignore = "needs oci registry"]
fn test_install_and_uninstall_with_oci_registry() {
    const ARTIFACT_CONTENT: &str = "some-content";

    let (_artifact_digest, reference) = push_artifact(ARTIFACT_CONTENT, REGISTRY_URL);

    let temp_dir = tempfile::tempdir().unwrap();
    let base_path = temp_dir.path().to_path_buf();

    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap(),
    );

    let downloader = OCIRefDownloader::try_new(
        ProxyConfig::default(),
        runtime,
        Some(ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        }),
    )
    .unwrap();

    let package_manager = OCIPackageManager {
        downloader,
        directory_manager: DirectoryManagerFs,
        file_manager: LocalFile,
        base_path: base_path.clone(),
    };

    let agent_id = AgentID::try_from("test-agent").unwrap();

    // Install
    let installed_path = package_manager.install(&agent_id, reference.clone());

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
    // The path should be base_path/agent_id/packages/repo_tag
    let repo = reference.repository();
    let tag = reference.tag().unwrap_or("latest");
    let expected_filename = format!("{}_{}", repo, tag).replace("/", "_");

    let expected_path = base_path
        .join(&agent_id)
        .join("packages")
        .join(expected_filename);

    assert_eq!(installed_path, expected_path);

    // Uninstall
    package_manager
        .uninstall(&agent_id, installed_path.clone())
        .unwrap();
    assert!(!installed_path.exists());
}
