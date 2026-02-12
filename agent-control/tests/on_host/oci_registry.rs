use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::oci_artifact::push_agent_package;
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use httpmock::{MockServer, When};
use newrelic_agent_control::agent_control::run::on_host::OCI_TEST_REGISTRY_URL;
use newrelic_agent_control::http::client::HttpClient;
use newrelic_agent_control::http::config::{HttpConfig, ProxyConfig};
use newrelic_agent_control::oci;
use newrelic_agent_control::package::oci::artifact_definitions::PackageMediaType;
use newrelic_agent_control::package::oci::downloader::{OCIAgentDownloader, OCIArtifactDownloader};
use newrelic_agent_control::signature::public_key_fetcher::PublicKeyFetcher;
use oci_client::client::{ClientConfig, ClientProtocol};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::tempdir;

fn create_client_with_proxy(proxy_config: ProxyConfig) -> oci::Client {
    let http_client = HttpClient::new(HttpConfig::default()).unwrap();
    let fetcher = PublicKeyFetcher::new(http_client);

    oci::Client::try_new(
        ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        },
        proxy_config,
        fetcher,
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

    let (artifact_digest, reference) = push_agent_package(
        &file_to_push,
        OCI_TEST_REGISTRY_URL,
        PackageMediaType::AgentPackageLayerTarGz,
    );

    let temp_dir = tempdir().unwrap();
    let local_agent_data_dir = temp_dir.path();

    let runtime = tokio_runtime();

    let client = create_client_with_proxy(ProxyConfig::default());

    let downloader = OCIArtifactDownloader::new(client, runtime);

    let _ = downloader
        .download(&reference, local_agent_data_dir)
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

    let (artifact_digest, reference) = push_agent_package(
        &file_to_push,
        OCI_TEST_REGISTRY_URL,
        PackageMediaType::AgentPackageLayerTarGz,
    );

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

    let proxy_config = serde_yaml::from_str::<ProxyConfig>(&proxy_yaml).unwrap();

    let runtime = tokio_runtime();

    let client = create_client_with_proxy(proxy_config);

    let downloader =
        OCIArtifactDownloader::new(client, runtime).with_retries(4, Duration::from_millis(100));

    let result = downloader.download(&reference, local_agent_data_dir);
    assert!(result.is_ok());

    // Verify that the expected files were created by digest and media type
    let file_path = local_agent_data_dir.join(artifact_digest.replace(':', "_"));
    assert!(file_path.exists());
}
