use crate::common::runtime::block_on;
use oci_client::client::{ClientConfig, ClientProtocol, Config, ImageLayer};
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, annotations, manifest};
use oci_spec::distribution::Reference;
use std::collections::BTreeMap;
use std::error::Error;
use std::time::SystemTime;
use tempfile::TempDir;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

///run_tag creates the tag used for pushing the artifact based on the actual timestamp to be unique
fn run_tag() -> String {
    let now = SystemTime::now();

    let duration = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("SystemTime went backwards");

    format!("{}", duration.as_nanos())
}

/// push_artifact pushes the provided artifact and reference to the oci registry provided on the
/// reference, it returns the digest of the artifact or panics if it fails.
pub fn push_artifact(artifact: &str, registry_url: &str) -> (String, Reference) {
    block_on(async {
        let reference =
            Reference::try_from(format!("{}/test:{}", registry_url, run_tag())).unwrap();
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join(artifact);

        let _ = std::fs::write(file_path.clone(), artifact);

        let oci_client = Client::new(ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        });

        let mut file = File::open(&file_path).await.unwrap();

        let mut blob_data = Vec::new();
        file.read_to_end(&mut blob_data).await.unwrap();

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
            .push(
                &reference,
                &layers,
                config,
                &RegistryAuth::Anonymous,
                Some(image_manifest),
            )
            .await
            .map(|push_response| push_response.manifest_url)
            .unwrap();

        (
            fetch_manifest_and_get_digest(oci_client, &reference)
                .await
                .unwrap(),
            reference,
        )
    })
}

async fn fetch_manifest_and_get_digest(
    oci_client: Client,
    reference: &Reference,
) -> Result<String, Box<dyn Error>> {
    let (manifest, _) = oci_client
        .pull_image_manifest(reference, &RegistryAuth::Anonymous)
        .await?;

    // Iterate over layers to find the one with the specified title annotation
    for layer in manifest.layers {
        if let Some(annotations) = &layer.annotations
            && let Some(_layer_title) = annotations.get("org.opencontainers.image.title")
        {
            return Ok(layer.digest.clone());
        }
    }

    Err(Box::<dyn Error>::from(
        "Digest for artifact not found".to_string(),
    ))
}
