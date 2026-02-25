use crate::common::oci::{hex_bytes, push_platform_config_descriptor};
use crate::common::runtime::block_on;
use aws_lc_rs::digest::{SHA256, digest};
use newrelic_agent_control::package::oci::artifact_definitions::{
    LayerMediaType, ManifestArtifactType, PackageMediaType,
};
use oci_client::Reference;
use oci_client::client::{ClientConfig, ClientProtocol};
use oci_client::config::{Architecture, Os};
use oci_client::manifest::{
    ImageIndexEntry, OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MEDIA_TYPE, OciDescriptor,
    OciImageIndex, OciImageManifest, Platform,
};
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, annotations, manifest};
use std::backtrace::Backtrace;
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

/// Pushes the provided artifact to the OCI registry and returns the blob digest and the reference
/// to the pushed index manifest. The structure is a manifest index (multiarch) with a single entry
/// for the current platform.
pub fn push_agent_package(
    file_to_push: &PathBuf,
    registry_url: &str,
    media_type: PackageMediaType,
) -> (String, Reference) {
    block_on(async {
        let tag = testing_unique_tag();
        let index_reference = Reference::try_from(format!("{registry_url}/test:{tag}")).unwrap();

        let oci_client = Client::new(ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        });

        let file_name = file_to_push
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        let blob_descriptor =
            push_package_blob(&oci_client, &index_reference, file_to_push, media_type).await;
        let blob_digest = blob_descriptor.digest.clone();

        let (manifest_digest, manifest_size) =
            push_package_manifest(&oci_client, &index_reference, blob_descriptor, file_name).await;

        push_package_index(
            &oci_client,
            &index_reference,
            manifest_digest,
            manifest_size,
        )
        .await;

        (blob_digest, index_reference)
    })
}

/// Creates a tag to be used when pushing OCI artifacts to the testing server.
/// The tag is built using the [Backtrace] so it is expected to be different for
/// different tests.
fn testing_unique_tag() -> String {
    let backtrace = Backtrace::force_capture().to_string();
    let mut hasher = DefaultHasher::new();
    backtrace.hash(&mut hasher);
    let hash = hasher.finish();
    format!("{hash}")
}

/// Reads the file at `file_to_push`, pushes it as a blob to the registry, and returns its
/// descriptor.
async fn push_package_blob(
    oci_client: &Client,
    reference: &Reference,
    file_to_push: &PathBuf,
    media_type: PackageMediaType,
) -> OciDescriptor {
    let mut file = File::open(file_to_push).await.unwrap();
    let mut blob_data = Vec::new();
    file.read_to_end(&mut blob_data).await.unwrap();

    let blob_digest = format!(
        "sha256:{}",
        hex_bytes(digest(&SHA256, blob_data.as_slice()).as_ref())
    );
    let size = blob_data.len() as i64;

    oci_client
        .push_blob(reference, blob_data, blob_digest.as_str())
        .await
        .unwrap();

    OciDescriptor {
        media_type: LayerMediaType::AgentPackage(media_type).to_string(),
        digest: blob_digest,
        size,
        ..Default::default()
    }
}

/// Builds and pushes an [OciImageManifest] containing `blob_descriptor` as its single layer.
/// Returns the canonical manifest digest (as reported by the registry) and the serialized size.
async fn push_package_manifest(
    oci_client: &Client,
    index_reference: &Reference,
    blob_descriptor: OciDescriptor,
    file_name: String,
) -> (String, i64) {
    // The manifest is pushed under a tagged reference because the client's local digest
    // calculation does not always match the registry's canonical JSON. The tag is not used in
    // production scenarios.
    let manifest_reference = Reference::try_from(format!("{index_reference}-manifest")).unwrap();

    let mut title_annotation: BTreeMap<String, String> = BTreeMap::new();
    title_annotation.insert(
        annotations::ORG_OPENCONTAINERS_IMAGE_TITLE.to_string(),
        file_name,
    );

    let pkg_manifest = OciImageManifest {
        media_type: Some(OCI_IMAGE_MEDIA_TYPE.to_string()),
        artifact_type: Some(ManifestArtifactType::AgentPackage.to_string()),
        layers: vec![blob_descriptor],
        config: push_platform_config_descriptor(oci_client, index_reference).await,
        annotations: Some(title_annotation),
        ..Default::default()
    };

    let manifest_size = serde_json::to_vec(&pkg_manifest).unwrap().len() as i64;

    oci_client
        .push_manifest(
            &manifest_reference,
            &manifest::OciManifest::Image(pkg_manifest),
        )
        .await
        .unwrap();

    // Fetch the digest as stored by the registry (canonical JSON may differ from local serialization).
    let manifest_digest = oci_client
        .fetch_manifest_digest(&manifest_reference, &RegistryAuth::Anonymous)
        .await
        .unwrap();

    (manifest_digest, manifest_size)
}

/// Builds and pushes an [OciImageIndex] with a single entry pointing at the given manifest,
/// mimicking the structure of a real multi-platform package.
async fn push_package_index(
    oci_client: &Client,
    index_reference: &Reference,
    manifest_digest: String,
    manifest_size: i64,
) {
    let image_index = OciImageIndex {
        schema_version: 2,
        media_type: Some(OCI_IMAGE_INDEX_MEDIA_TYPE.to_string()),
        artifact_type: None,
        manifests: vec![ImageIndexEntry {
            media_type: OCI_IMAGE_MEDIA_TYPE.to_string(),
            digest: manifest_digest,
            size: manifest_size,
            platform: Some(Platform {
                architecture: Architecture::default(),
                os: Os::default(),
                os_version: None,
                os_features: None,
                variant: None,
                features: None,
            }),
            annotations: None,
        }],
        annotations: None,
    };

    oci_client
        .push_manifest(
            index_reference,
            &manifest::OciManifest::ImageIndex(image_index),
        )
        .await
        .unwrap();
}
