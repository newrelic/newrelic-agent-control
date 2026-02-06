//! This module provides an [oci_client] wrapper.
//! It provides an unified way to build the client and additional features required by Agent Control's
//! usage of OCI repositories.

use oci_client::{
    Reference,
    client::{AsLayerDescriptor, ClientConfig},
    manifest::OciImageManifest,
    secrets::RegistryAuth,
};
use tokio::io::AsyncWrite;

use crate::http::config::ProxyConfig;

mod error;
mod proxy;

pub use error::OciClientError;

/// [oci_client::Client] wrapper with extended functionality.
#[derive(Clone)]
pub struct Client {
    client: oci_client::Client,
    auth: RegistryAuth,
}

impl Client {
    /// Returns a new client with the provided configuration and [RegistryAuth::Anonymous] authentication.
    pub fn try_new(
        proxy_config: ProxyConfig,
        client_config: ClientConfig,
    ) -> Result<Self, OciClientError> {
        let client_config = proxy::setup_proxy(client_config, proxy_config)?;
        Ok(Self {
            client: oci_client::Client::new(client_config),
            auth: RegistryAuth::Anonymous,
        })
    }

    /// Sets up the provided authentication.
    pub fn with_auth(self, auth: RegistryAuth) -> Self {
        Self { auth, ..self }
    }

    /// Calls [oci_client::Client::pull_image_manifest] using the configured auth.
    pub async fn pull_image_manifest(
        &self,
        reference: &Reference,
    ) -> Result<(OciImageManifest, String), OciClientError> {
        self.client
            .pull_image_manifest(reference, &self.auth)
            .await
            .map_err(|err| OciClientError::PullManifest(err.into()))
    }

    /// Calls [oci_client::Client::pull_blob].
    pub async fn pull_blob<T: AsyncWrite>(
        &self,
        reference: &Reference,
        layer: impl AsLayerDescriptor,
        out: T,
    ) -> Result<(), OciClientError> {
        self.client
            .pull_blob(reference, layer, out)
            .await
            .map_err(|err| OciClientError::PullBlob(err.into()))
    }
}

#[cfg(test)]
pub mod tests {
    use std::str::FromStr;

    use httpmock::{Method::GET, MockServer};
    use oci_client::{
        Reference,
        client::ClientConfig,
        manifest::{OciDescriptor, OciImageManifest},
    };
    use ring::digest::{SHA256, digest};
    use tempfile::tempdir;

    use crate::{http::config::ProxyConfig, oci::Client};

    /// Simple test to show the client and the mock server usage.
    #[tokio::test]
    async fn test_client() {
        let server = FakeOciServer::new("repo", "v1.2.3")
            .with_layer(b"some content", "fake-media_type")
            .build();

        let client = Client::try_new(
            ProxyConfig::default(),
            ClientConfig {
                protocol: oci_client::client::ClientProtocol::Http,
                ..Default::default()
            },
        )
        .unwrap();

        let reference = &server.reference();

        let (image_manifest, _) = client.pull_image_manifest(reference).await.unwrap();

        let layer = &image_manifest.layers[0];
        assert_eq!(layer.media_type, "fake-media_type");

        let tmp = tempdir().unwrap();
        let filepath = tmp.path().join(&layer.digest);
        let mut file = tokio::fs::File::create(&filepath).await.unwrap();

        client.pull_blob(reference, layer, &mut file).await.unwrap();
        file.sync_data().await.unwrap();

        assert_eq!(tokio::fs::read(filepath).await.unwrap(), b"some content");
    }

    /// Mock OCI server for testing
    pub struct FakeOciServer {
        server: MockServer,
        repo: String,
        tag: String,
        layers: Vec<(String, Vec<u8>)>, // (digest, content)
        manifest: OciImageManifest,
    }

    impl FakeOciServer {
        pub fn new(repo: &str, tag: &str) -> Self {
            Self {
                server: MockServer::start(),
                repo: repo.to_string(),
                tag: tag.to_string(),
                layers: Vec::new(),
                manifest: OciImageManifest::default(),
            }
        }

        pub fn with_artifact_type(mut self, artifact_type: &str) -> Self {
            self.manifest.artifact_type = Some(artifact_type.to_string());
            self
        }

        pub fn with_layer(mut self, content: &[u8], media_type: &str) -> Self {
            let digest = digest(&SHA256, content);
            let digest_str = format!("sha256:{}", hex_bytes(digest.as_ref()));
            self.layers.push((digest_str, content.to_vec()));

            let layer_descriptor = OciDescriptor {
                media_type: media_type.to_string(),
                digest: self.layers.last().unwrap().0.clone(),
                size: content.len() as i64,
                ..Default::default()
            };
            self.manifest.layers.push(layer_descriptor);
            self
        }

        pub fn build(self) -> Self {
            self.setup_mocks();
            self
        }

        pub fn setup_mocks(&self) {
            // Mock manifest endpoint
            let manifest_clone = self.manifest.clone();
            self.server.mock(|when, then| {
                when.method(GET)
                    .path(format!("/v2/{}/manifests/{}", self.repo, self.tag));
                then.status(200)
                    .header("Content-Type", "application/vnd.oci.image.manifest.v1+json")
                    .json_body_obj(&manifest_clone);
            });

            // Mock blob endpoints
            for (digest, content) in &self.layers {
                let content_clone = content.clone();
                let digest_clone = digest.clone();
                self.server.mock(move |when, then| {
                    when.method(GET)
                        .path(format!("/v2/{}/blobs/{}", self.repo, digest_clone));
                    then.status(200)
                        .header("Content-Type", "application/octet-stream")
                        .body(&content_clone);
                });
            }
        }

        pub fn reference(&self) -> Reference {
            Reference::from_str(&format!(
                "{}/{}:{}",
                self.server.address(),
                self.repo,
                self.tag
            ))
            .unwrap()
        }
    }

    fn hex_bytes(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
