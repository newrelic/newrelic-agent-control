use crate::common::runtime::block_on;
use newrelic_agent_control::package::oci::artifact_definitions::{
    LayerMediaType, ManifestArtifactType, PackageMediaType,
};
use oci_client::client::{ClientConfig, ClientProtocol};
use oci_client::manifest::{OCI_IMAGE_MEDIA_TYPE, OciDescriptor, OciImageManifest};
use oci_client::{Client, annotations, manifest};
use oci_spec::distribution::Reference;
use ring::digest::{SHA256, digest};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::SystemTime;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

// Registry created in the make target executing oci-registry.sh
pub const REGISTRY_URL: &str = "localhost:5001";

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
pub fn push_agent_package(
    file_to_push: &PathBuf,
    registry_url: &str,
    media_type: PackageMediaType,
) -> (String, Reference) {
    block_on(async {
        let reference =
            Reference::try_from(format!("{}/test:{}", registry_url, run_tag())).unwrap();
        let blob_reference = Reference::try_from(format!("{}/test", registry_url)).unwrap();

        let oci_client = Client::new(ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        });

        let mut file = File::open(file_to_push).await.unwrap();

        let mut blob_data = Vec::new();
        file.read_to_end(&mut blob_data).await.unwrap();

        let file_name = file_to_push
            .file_name()
            .map(|os_str| os_str.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown_file".to_string());

        let mut annotations: BTreeMap<String, String> = BTreeMap::new();
        annotations.insert(
            annotations::ORG_OPENCONTAINERS_IMAGE_TITLE.to_string(),
            file_name,
        );

        let blob_digest = format!(
            "sha256:{}",
            hex_bytes(digest(&SHA256, blob_data.as_slice()).as_ref())
        );

        oci_client
            .push_blob(&blob_reference, &blob_data, blob_digest.as_str())
            .await
            .unwrap();

        let blob_descriptor = OciDescriptor {
            media_type: LayerMediaType::AgentPackage(media_type).to_string(),
            digest: blob_digest.clone(),
            size: blob_data.len() as i64,
            ..Default::default()
        };

        // Push empty config blob (required for OCI artifacts)
        let empty_config = b"{}";
        let empty_config_digest = format!(
            "sha256:{}",
            hex_bytes(digest(&SHA256, empty_config).as_ref())
        );
        oci_client
            .push_blob(&blob_reference, empty_config, empty_config_digest.as_str())
            .await
            .unwrap();

        let image_manifest = OciImageManifest {
            media_type: Some(OCI_IMAGE_MEDIA_TYPE.to_string()),
            artifact_type: Some(ManifestArtifactType::AgentPackage.to_string()),
            layers: vec![blob_descriptor],
            config: OciDescriptor {
                media_type: "application/vnd.oci.empty.v1+json".to_string(),
                digest: empty_config_digest.clone(),
                size: empty_config.len() as i64,
                ..Default::default()
            },
            annotations: Some(annotations),
            ..Default::default()
        };

        oci_client
            .push_manifest(&reference, &manifest::OciManifest::Image(image_manifest))
            .await
            .unwrap();

        (blob_digest, reference)
    })
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut hex_string = String::new();
    for byte in bytes {
        hex_string.push_str(&format!("{:02x}", byte));
    }
    hex_string
}
