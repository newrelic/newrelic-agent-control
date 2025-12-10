use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use oci_client::{annotations, manifest, Client};
use oci_client::client::{ClientConfig, ClientProtocol, Config, ImageLayer};
use oci_spec::distribution::Reference;
use tempfile::TempDir;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
#[cfg(test)]
use newrelic_agent_control::http::config::ProxyConfig;
use newrelic_agent_control::packages::oci::downloader::OCIDownloader;
use sha2::{Sha256, Digest};
use hex;
use httpmock::{MockServer, When};
use oci_client::secrets::RegistryAuth;
use rand::distr::Alphanumeric;
use rand::Rng;
use thiserror::Error;
use tokio::runtime::Runtime;

#[derive(Debug, Error)]
#[error("{0}")]
struct DigestNotfoundError(String);

fn compute_digest(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    format!("sha256:{}", hex::encode(result))
}

async fn push_artifact(artifact: &str, reference: &Reference) -> Result<String, Box<dyn Error>>  {
    let dir = TempDir::new().unwrap();
    let file_path = dir
        .path()
        .join(artifact);

    let _ = std::fs::write(
        file_path.clone(),
        artifact,
    );

    let oci_client = Client::new(ClientConfig {
        protocol: ClientProtocol::Http,
        ..Default::default()
    });

    let mut file = File::open(&file_path).await?;

    let mut blob_data = Vec::new();
    file.read_to_end(&mut blob_data).await?;

    let mut annotations: BTreeMap<String, String> = BTreeMap::new();
    annotations.insert(
        annotations::ORG_OPENCONTAINERS_IMAGE_TITLE.to_string(),
        artifact.to_string(),
    );

    let layers = vec![ImageLayer::new(
        blob_data.clone(),
        manifest::IMAGE_LAYER_GZIP_MEDIA_TYPE.to_string(),
        Some(annotations),
    )];

    let config = Config {
        data: blob_data,
        media_type: manifest::IMAGE_CONFIG_MEDIA_TYPE.to_string(),
        annotations: None,
    };

    let image_manifest = manifest::OciImageManifest::build(&layers, &config, None);

    let _ = oci_client
        .push(reference, &layers, config, &RegistryAuth::Anonymous, Some(image_manifest))
        .await
        .map(|push_response| push_response.manifest_url)?;

    fetch_manifest_and_get_digest(oci_client, reference).await
}

async fn fetch_manifest_and_get_digest(oci_client: Client, reference: &Reference) -> Result<String, Box<dyn Error>> {
    let (manifest, _) = oci_client
        .pull_image_manifest(reference, &RegistryAuth::Anonymous)
        .await?;

    // Iterate over layers to find the one with the specified title annotation
    for layer in manifest.layers {
        if let Some(annotations) = &layer.annotations {
            if let Some(_layer_title) = annotations.get("org.opencontainers.image.title") {
                return Ok(layer.digest.clone());
            }
        }
    }

    Err(Box::new(DigestNotfoundError("Digest for artifact not found".to_string())))
}

fn runtime_and_run_tag() -> (Arc<Runtime>, String) {
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build().unwrap()
    );

    let rng = rand::rng();
    let run_tag: String = rng.sample_iter(&Alphanumeric)
        .take(7)
        .map(char::from)
        .collect();
    (runtime, run_tag)
}

#[test]
fn test_download_artifact_from_local_registry() {
    let (runtime, run_tag) = runtime_and_run_tag();

    let reference = Reference::try_from(format!("localhost:5001/test:{}", run_tag)).unwrap();
    const ARTIFACT: &str = "artifact.txt";

    let push_result = runtime.block_on(
        push_artifact(ARTIFACT, &reference)
    );
    assert!(push_result.is_ok());
    let artifact_digest = push_result.ok().unwrap();

    let temp_dir = tempfile::tempdir().unwrap();
    let local_agent_data_dir = temp_dir.path().to_path_buf();

    let downloader = OCIDownloader::try_new(
        ProxyConfig::default(),
        runtime,
        Some(ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        }),
    );

    let result = downloader.unwrap().download_artifact(&reference, local_agent_data_dir.clone());
    assert!(result.is_ok());

    // Verify that the expected files were created by digest and media type
    let file_path = local_agent_data_dir.join(artifact_digest);
    assert!(file_path.exists());
}

#[test]
fn test_download_artifact_from_local_registry_using_proxy_with_retries() {
    let (runtime, run_tag) = runtime_and_run_tag();

    let reference = Reference::try_from(format!("localhost:5001/test:{}", run_tag)).unwrap();

    const ARTIFACT: &str = "artifact.txt";

    let push_result = runtime.block_on(
        push_artifact(ARTIFACT, &reference)
    );

    assert!(push_result.is_ok());

    // Proxy server will request the target server, allowing requests to that host only
    let proxy_server = MockServer::start();
    let attempts = Arc::new(Mutex::new(0));

    let attempts_clone = Arc::clone(&attempts);

    proxy_server.proxy(|rule| {
        rule.filter(|when| {
            when.host("localhost").port(5001).and(|when|-> When {
                when.is_true(move |_|{
                    let mut attempts = attempts_clone.lock().unwrap();
                    *attempts += 1;
                    // it makes 2 calls per request
                    *attempts > 7
                })
            });
        });
    });

    let reference = Reference::try_from(format!("localhost:5001/test:{}", run_tag)).unwrap();
    let artifact_digest = push_result.ok().unwrap();

    let temp_dir = tempfile::tempdir().unwrap();
    let local_agent_data_dir = temp_dir.path().to_path_buf();

    let proxy_url = proxy_server.base_url();
    let proxy_yaml =format!(
        "{{\"url\": \"{proxy_url}\"}}"
    );

    let proxy_config = serde_yaml::from_str::<ProxyConfig>(&proxy_yaml).unwrap();

    let downloader = OCIDownloader::try_new(
        proxy_config,
        runtime,
        Some(ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        }),
    )
        .unwrap()
        .with_retries(4, Duration::from_millis(100));


    let result = downloader.download_artifact(&reference, local_agent_data_dir.clone());
    assert!(result.is_ok());


    // Verify that the expected files were created by digest and media type
    let file_path = local_agent_data_dir.join(artifact_digest);
    assert!(file_path.exists());
}
