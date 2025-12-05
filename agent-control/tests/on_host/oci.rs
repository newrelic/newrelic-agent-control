use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use oci_client::{annotations, manifest, Client};
use oci_client::client::{ClientConfig, ClientProtocol, Config, ImageLayer};
use oci_spec::distribution::Reference;
use tempfile::TempDir;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use newrelic_agent_control::http::config::ProxyConfig;
use newrelic_agent_control::packages::oci::downloader::OCIDownloader;
use sha2::{Sha256, Digest};
use hex;
use oci_client::secrets::RegistryAuth;
use rand::distr::Alphanumeric;
use rand::Rng;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("{0}")]
struct DigestNotfoundError(String);

fn compute_digest(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    format!("sha256:{}", hex::encode(result))
}

async fn push_artifact(blob_path: PathBuf, run_tag:String) -> Result<String, Box<dyn Error>>  {
    let oci_client = Client::new(ClientConfig {
        protocol: ClientProtocol::Http,
        ..Default::default()
    });
    let reference = Reference::try_from(format!("localhost:5001/test:{}", run_tag))?;

    let mut file = File::open(&blob_path).await?;

    let mut blob_data = Vec::new();
    file.read_to_end(&mut blob_data).await?;

    let mut annotations: BTreeMap<String, String> = BTreeMap::new();
    annotations.insert(
        annotations::ORG_OPENCONTAINERS_IMAGE_TITLE.to_string(),
        "artifact.txt".to_string(),
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

    let push_result = oci_client
        .push(&reference, &layers, config, &RegistryAuth::Anonymous, Some(image_manifest))
        .await
        .map(|push_response| push_response.manifest_url)?;

    fetch_manifest_and_get_digest(oci_client, run_tag).await
}

async fn fetch_manifest_and_get_digest(oci_client: Client, run_tag: String) -> Result<String, Box<dyn Error>> {
    // Create a reference using the digest
    let reference = Reference::try_from(format!("localhost:5001/test:{}", run_tag)).unwrap();

    // Fetch the manifest
    let (manifest, _) = oci_client
        .pull_image_manifest(&reference, &RegistryAuth::Anonymous)
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

#[test]
fn test_download_artifact_from_local_registry() {
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build().unwrap()
    );

    let rng = rand::rng();
    let run_tag:String = rng.sample_iter(&Alphanumeric)
        .take(7)
        .map(char::from)
        .collect();

    const ARTIFACT: &str = "artifact.txt";

    let dir = TempDir::new().unwrap();
    let file_path = dir
        .path()
        .join(ARTIFACT);

    let _ = std::fs::write(
        file_path.clone(),
        ARTIFACT,
    );

    let client_config = ClientConfig {
        protocol: ClientProtocol::Http,
        ..Default::default()
    };

    let push_result = runtime.block_on(
        push_artifact(file_path, run_tag.clone())
    );

    assert!(push_result.is_ok());

    let artifact_digest = push_result.ok().unwrap();

    // Setup a temporary directory for testing
    let temp_dir = tempfile::tempdir().unwrap();
    let local_agent_data_dir = temp_dir.path().to_path_buf();

    // Create an instance of OCIDownloader
    let downloader = OCIDownloader::try_new(
        ProxyConfig::default(),
        runtime,
        Some(client_config),
    );

    // Reference to the image in the local ORAS registry
    let reference = Reference::try_from(format!("localhost:5001/test:{}", run_tag)).unwrap();

    // Call the download_artifact method
    let result = downloader.unwrap().download_artifact(reference.clone(), local_agent_data_dir.clone());

    // Assert that the download was successful
    assert!(result.is_ok());

    // Verify that the expected files were created by digest and media type
    let file_path = local_agent_data_dir.join(artifact_digest);
    assert!(file_path.exists());

    //Read also contents?
}

