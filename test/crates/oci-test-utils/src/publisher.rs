use crate::LOCAL_HTTP_REGISTRY_URL;
use crate::blob_digest;
use oci_client::Client;
use oci_client::Reference;
use oci_client::annotations;
use oci_client::client::{ClientConfig, ClientProtocol};
use oci_client::config::{Architecture, Os};
use oci_client::manifest;
use oci_client::manifest::{
    IMAGE_CONFIG_MEDIA_TYPE, ImageIndexEntry, OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MEDIA_TYPE,
    OciDescriptor, OciImageIndex, OciImageManifest, Platform,
};
use oci_client::secrets::RegistryAuth;
use std::backtrace::Backtrace;
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::runtime::Handle;

const AGENT_PACKAGE_MANIFEST_ARTIFACT_TYPE: &str = "application/vnd.newrelic.agent.v1";
const AGENT_PACKAGE_LAYER_TAR_GZ: &str = "application/vnd.newrelic.agent.content.v1.tar+gzip";
const AGENT_PACKAGE_LAYER_ZIP: &str = "application/vnd.newrelic.agent.content.v1.zip";

const REPOSITORY_NAME: &str = "test";

pub enum PackageMediaType {
    TarGz,
    Zip,
}

impl PackageMediaType {
    fn layer_media_type(&self) -> &'static str {
        match self {
            PackageMediaType::TarGz => AGENT_PACKAGE_LAYER_TAR_GZ,
            PackageMediaType::Zip => AGENT_PACKAGE_LAYER_ZIP,
        }
    }
}

pub struct PackagePublisher {
    registry_url: String,
    runtime_handle: Handle,
    client: Client,
}

impl PackagePublisher {
    pub fn new(runtime_handle: Handle, registry_url: impl Into<String>) -> Self {
        Self {
            registry_url: registry_url.into(),
            runtime_handle,
            client: Client::new(ClientConfig {
                protocol: ClientProtocol::HttpsExcept(vec![LOCAL_HTTP_REGISTRY_URL.to_string()]),
                ..Default::default()
            }),
        }
    }

    pub fn with_basic_auth(self, user: &str, pass: &str) -> Self {
        self.runtime_handle
            .block_on(self.client.auth(
                &Reference::with_tag(
                    self.registry_url.clone(),
                    REPOSITORY_NAME.to_string(),
                    String::new(),
                ),
                &RegistryAuth::Basic(user.to_string(), pass.to_string()),
                oci_client::RegistryOperation::Push,
            ))
            .unwrap();
        self
    }

    /// Pushes `file` as an OCI agent package artifact and returns the index manifest reference.
    /// The artifact is structured as a manifest index (multiarch) with a single entry for the
    /// current platform.
    pub fn push(&self, file: &Path, media_type: PackageMediaType) -> Reference {
        self.push_with_tag(file, media_type, &unique_tag())
    }

    /// Same as [`push`] but uses `tag` instead of a generated unique tag.
    pub fn push_with_tag(&self, file: &Path, media_type: PackageMediaType, tag: &str) -> Reference {
        self.runtime_handle
            .block_on(async { self.push_async(file, media_type, tag).await })
    }

    async fn push_async(&self, file: &Path, media_type: PackageMediaType, tag: &str) -> Reference {
        let index_reference: Reference = format!("{}/{REPOSITORY_NAME}:{tag}", self.registry_url)
            .parse()
            .unwrap();

        let file_name = file.file_name().unwrap().to_string_lossy().to_string();

        let blob_descriptor = self.push_blob(&index_reference, file, &media_type).await;

        let (manifest_digest, manifest_size) = self
            .push_package_manifest(&index_reference, blob_descriptor, file_name)
            .await;

        self.push_package_index(&index_reference, manifest_digest, manifest_size)
            .await;

        index_reference
    }

    async fn push_package_manifest(
        &self,
        index_reference: &Reference,
        blob_descriptor: OciDescriptor,
        file_name: String,
    ) -> (String, i64) {
        // Pushed under a tagged reference because the client's local digest calculation does not
        // always match the registry's canonical JSON. The tag is not used in production scenarios.
        let manifest_reference: Reference = format!("{index_reference}-manifest").parse().unwrap();

        let mut title_annotation: BTreeMap<String, String> = BTreeMap::new();
        title_annotation.insert(
            annotations::ORG_OPENCONTAINERS_IMAGE_TITLE.to_string(),
            file_name,
        );

        let config = self.push_platform_config(index_reference).await;

        let pkg_manifest = OciImageManifest {
            media_type: Some(OCI_IMAGE_MEDIA_TYPE.to_string()),
            artifact_type: Some(AGENT_PACKAGE_MANIFEST_ARTIFACT_TYPE.to_string()),
            layers: vec![blob_descriptor],
            config,
            annotations: Some(title_annotation),
            ..Default::default()
        };

        let manifest_size = serde_json::to_vec(&pkg_manifest).unwrap().len() as i64;

        self.client
            .push_manifest(
                &manifest_reference,
                &manifest::OciManifest::Image(pkg_manifest),
            )
            .await
            .unwrap();

        let manifest_digest = self
            .client
            .fetch_manifest_digest(&manifest_reference, &RegistryAuth::Anonymous)
            .await
            .unwrap();

        (manifest_digest, manifest_size)
    }

    async fn push_platform_config(&self, reference: &Reference) -> OciDescriptor {
        let config_bytes: Vec<u8> = serde_json::to_vec(&serde_json::json!({
            "architecture": &Architecture::default(),
            "os": &Os::default(),
        }))
        .unwrap();

        let digest = blob_digest(&config_bytes);
        let size = config_bytes.len() as i64;

        self.client
            .push_blob(reference, config_bytes, &digest)
            .await
            .unwrap();

        OciDescriptor {
            media_type: IMAGE_CONFIG_MEDIA_TYPE.to_string(),
            digest,
            size,
            ..Default::default()
        }
    }

    async fn push_package_index(
        &self,
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
                artifact_type: None,
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

        self.client
            .push_manifest(
                index_reference,
                &manifest::OciManifest::ImageIndex(image_index),
            )
            .await
            .unwrap();
    }

    async fn push_blob(
        &self,
        reference: &Reference,
        file: &Path,
        media_type: &PackageMediaType,
    ) -> OciDescriptor {
        let mut f = File::open(file).await.unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).await.unwrap();

        let digest = blob_digest(&data);
        let size = data.len() as i64;

        self.client
            .push_blob(reference, data, &digest)
            .await
            .unwrap();

        OciDescriptor {
            media_type: media_type.layer_media_type().to_string(),
            digest,
            size,
            ..Default::default()
        }
    }
}

/// Creates a tag to be used when pushing OCI artifacts to the testing server.
/// The tag is built using the [Backtrace] so it is expected to be different for
/// different tests.
fn unique_tag() -> String {
    let backtrace = Backtrace::force_capture().to_string();
    let mut hasher = DefaultHasher::new();
    backtrace.hash(&mut hasher);
    hasher.finish().to_string()
}
