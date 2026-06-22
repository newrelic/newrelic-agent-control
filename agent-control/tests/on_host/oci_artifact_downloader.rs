use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use httpmock::{MockServer, When};
use newrelic_agent_control::agent_control::config::Registry;
use newrelic_agent_control::agent_control::run::on_host::OCI_TEST_REGISTRY_URL;
use newrelic_agent_control::agent_type::runtime_config::on_host::package::rendered::{
    Oci, Repository, Version,
};
use newrelic_agent_control::http::config::ProxyConfig;
use newrelic_agent_control::oci;
use newrelic_agent_control::package::manager::PackageData;
use newrelic_agent_control::package::oci::downloader::{
    OCIPackageArtifactDownloader, OCIPackageDownloader,
};
use newrelic_agent_control::utils::retry::BackoffPolicy;
use oci_client::client::{ClientConfig, ClientProtocol};
use oci_test_utils::{PackageMediaType, PackagePublisher, blob_digest};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::tempdir;

fn create_client_with_proxy(proxy_config: ProxyConfig) -> oci::Client {
    oci::Client::try_new(
        ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        },
        proxy_config,
        tokio_runtime(),
    )
    .unwrap()
}
#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_download_artifact_from_local_registry_with_oci_registry() {
    let dir = tempdir().unwrap();
    let tmp_dir_to_compress = tempdir().unwrap();
    let file_to_push = dir.path().join("layer_digest.tar.gz");
    TestDataHelper::compress_tar_gz(
        tmp_dir_to_compress.path(),
        file_to_push.as_path(),
        "important content",
        "file1.txt",
    );

    let artifact_digest = blob_digest(&std::fs::read(&file_to_push).unwrap());
    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .push(&file_to_push, PackageMediaType::TarGz);

    let temp_dir = tempdir().unwrap();
    let local_agent_data_dir = temp_dir.path();

    let client = create_client_with_proxy(ProxyConfig::default());

    let downloader = OCIPackageArtifactDownloader::new(
        client,
        Registry::from_str(OCI_TEST_REGISTRY_URL).unwrap(),
        Default::default(),
        false,
    );

    let package_data = PackageData {
        id: "test-package".to_string(),
        oci: Oci {
            repository: Repository::from_str(reference.repository()).unwrap(),
            version: Version::from_str(reference.tag().unwrap()).unwrap(),
            public_key_url: None,
        },
        post_download_hook: None,
    };
    let _ = downloader
        .download(&package_data, local_agent_data_dir)
        .unwrap();

    // Verify that the expected files were created by digest and media type
    let file_path = local_agent_data_dir.join(artifact_digest.replace(':', "_"));
    assert!(file_path.exists());
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_download_artifact_from_local_registry_using_proxy_with_retries_with_oci_registry() {
    let dir = tempdir().unwrap();
    let tmp_dir_to_compress = tempdir().unwrap();
    let file_to_push = dir.path().join("layer_digest.tar.gz");
    TestDataHelper::compress_tar_gz(
        tmp_dir_to_compress.path(),
        file_to_push.as_path(),
        "important content",
        "file1.txt",
    );

    let artifact_digest = blob_digest(&std::fs::read(&file_to_push).unwrap());
    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .push(&file_to_push, PackageMediaType::TarGz);

    // Proxy server will request the target server, allowing requests to that host only
    let proxy_server = MockServer::start();
    let attempts = Arc::new(Mutex::new(0));

    let attempts_clone = Arc::clone(&attempts);

    // Proxy to the oci server only after 4 retries, the client makes 2 calls per time.
    proxy_server.proxy(|rule| {
        rule.filter(|when| {
            when.host("localhost").port(5001).and(|when| -> When {
                when.is_true(move |_| {
                    let mut attempts = attempts_clone.lock().unwrap();
                    *attempts += 1;
                    // it makes 2 calls per request
                    println!("Attempts remaining: {}", *attempts);
                    *attempts > 7
                })
            });
        });
    });

    let temp_dir = tempfile::tempdir().unwrap();
    let local_agent_data_dir = temp_dir.path();

    let proxy_url = proxy_server.base_url();
    let proxy_yaml = format!("{{\"url\": \"{proxy_url}\"}}");

    let proxy_config = serde_saphyr::from_str::<ProxyConfig>(&proxy_yaml).unwrap();

    let client = create_client_with_proxy(proxy_config);

    let downloader = OCIPackageArtifactDownloader::new(
        client,
        Registry::from_str(OCI_TEST_REGISTRY_URL).unwrap(),
        Default::default(),
        false,
    )
    .with_retry_policy(BackoffPolicy {
        max_attempts: 4,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(100),
        jitter: true,
    });

    let package_data = PackageData {
        id: "test-package".to_string(),
        oci: Oci {
            repository: Repository::from_str(reference.repository()).unwrap(),
            version: Version::from_str(reference.tag().unwrap()).unwrap(),
            public_key_url: None,
        },
        post_download_hook: None,
    };
    let result = downloader.download(&package_data, local_agent_data_dir);
    assert!(result.is_ok());

    // Verify that the expected files were created by digest and media type
    let file_path = local_agent_data_dir.join(artifact_digest.replace(':', "_"));
    assert!(file_path.exists());
}
