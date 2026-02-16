//! This module provides an [oci_client] wrapper.

use crate::http::config::ProxyConfig;
use oci_client::{
    Reference,
    client::{AsLayerDescriptor, ClientConfig},
    manifest::OciImageManifest,
    secrets::RegistryAuth,
};
use tokio::io::AsyncWrite;
use tracing::debug;

mod error;
mod proxy;
pub mod signature_verification;

use crate::oci::signature_verification::{
    fetch_trusted_signature_layers, triangulate, verify_signatures,
};
use crate::signature::public_key::PublicKey;
pub use error::OciClientError;

/// [oci_client::Client] wrapper with extended functionality.
#[derive(Clone)]
pub struct Client {
    client: oci_client::Client,
    auth: RegistryAuth,
}

impl Client {
    pub fn try_new(
        client_config: ClientConfig,
        proxy_config: ProxyConfig,
    ) -> Result<Self, OciClientError> {
        let client_config = proxy::setup_proxy(client_config, proxy_config)?;
        Ok(Self {
            client: oci_client::Client::new(client_config),
            auth: RegistryAuth::Anonymous,
        })
    }

    pub async fn pull_image_manifest(
        &self,
        reference: &Reference,
    ) -> Result<(OciImageManifest, String), OciClientError> {
        self.client
            .pull_image_manifest(reference, &self.auth)
            .await
            .map_err(|err| OciClientError::PullManifest(err.into()))
    }

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

    /// High-level method to ensure an image is signed before using it.
    pub async fn verify(
        &self,
        reference: &Reference,
        trusted_keys: &[PublicKey],
    ) -> Result<Reference, OciClientError> {
        // Resolve image digest (Client logic)
        let (_, digest) = self.pull_image_manifest(reference).await?;
        debug!("Image resolved to digest: {}", digest);

        // Calculate signature location (External logic)
        let signature_ref = triangulate(reference, &digest)?;
        debug!("Looking for signatures at: {}", signature_ref.whole());

        // Download signature layers (External logic, passing 'self')
        let layers = fetch_trusted_signature_layers(self, &signature_ref).await?;

        if layers.is_empty() {
            return Err(OciClientError::Verify(format!(
                "No signature layers found for image {}",
                reference.whole()
            )));
        }

        // Verify cryptography (External logic)
        verify_signatures(&layers, &digest, trusted_keys)?;

        Ok(Reference::with_digest(
            reference.registry().to_string(),
            reference.repository().to_string(),
            digest,
        ))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use base64::Engine;
    use httpmock::{Method::GET, MockServer};
    use oci_client::manifest::OciDescriptor;
    use ring::digest::{SHA256, digest};
    use std::collections::BTreeMap;
    use std::str::FromStr;
    use tempfile::tempdir;

    use crate::signature::public_key::tests::TestKeyPair;

    fn create_test_client() -> Client {
        Client::try_new(
            ClientConfig {
                protocol: oci_client::client::ClientProtocol::Http,
                ..Default::default()
            },
            ProxyConfig::default(),
        )
        .expect("Failed to create test Client")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_verify_full_flow_integration() {
        let kp = TestKeyPair::new(0);
        let pub_key = kp.public_key();

        // Setup Mock Server
        let app_server = FakeOciServer::new("my-app", "v1")
            .with_layer(b"my binary", "application/vnd.oci.image.layer.v1.tar+gzip");

        let manifest_bytes = serde_json::to_vec(&app_server.manifest).unwrap();
        let manifest_digest = digest(&SHA256, &manifest_bytes);
        let image_manifest_digest_str = format!("sha256:{}", hex_bytes(manifest_digest.as_ref()));

        let payload = serde_json::json!({
            "critical": { "identity": { "docker-reference": "" }, "image": { "docker-reference": image_manifest_digest_str }, "type": "cosign container image signature" },
            "optional": {}
        });
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(kp.sign(&payload_bytes));
        let sig_tag = format!("{}.sig", image_manifest_digest_str.replace(':', "-"));

        let sig_server =
            FakeOciServer::new("my-app", &sig_tag).with_cosign_layer(&payload_bytes, &sig_b64);

        let registry_mock = MockServer::start();
        app_server.setup_mocks_on(&registry_mock);
        sig_server.setup_mocks_on(&registry_mock);

        let client = create_test_client();
        let image_ref =
            Reference::from_str(&format!("{}/my-app:v1", registry_mock.address())).unwrap();

        let result = client.verify(&image_ref, &[pub_key]).await;

        assert!(result.is_ok(), "Error verifying: {:?}", result.err());
        let verified_ref = result.unwrap();
        assert!(verified_ref.whole().contains(&image_manifest_digest_str));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_client() {
        let server = FakeOciServer::new("repo", "v1.2.3")
            .with_layer(b"some content", "fake-media_type")
            .build();

        let client = create_test_client();
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_verify_fails_with_wrong_key() {
        let signer_kp = TestKeyPair::new(0);
        let trusted_kp = TestKeyPair::new(1);

        let app_server = FakeOciServer::new("my-app", "v1")
            .with_layer(b"binary", "application/vnd.oci.image.layer.v1.tar+gzip");

        let manifest_bytes = serde_json::to_vec(&app_server.manifest).unwrap();
        let manifest_digest = digest(&SHA256, &manifest_bytes);
        let image_manifest_digest_str = format!("sha256:{}", hex_bytes(manifest_digest.as_ref()));

        let payload = serde_json::json!({
            "critical": { "identity": { "docker-reference": "" }, "image": { "docker-reference": image_manifest_digest_str }, "type": "cosign container image signature" },
            "optional": {}
        });
        let payload_bytes = serde_json::to_vec(&payload).unwrap();

        let sig_b64 =
            base64::engine::general_purpose::STANDARD.encode(signer_kp.sign(&payload_bytes));
        let sig_tag = format!("{}.sig", image_manifest_digest_str.replace(':', "-"));

        let sig_server =
            FakeOciServer::new("my-app", &sig_tag).with_cosign_layer(&payload_bytes, &sig_b64);

        let registry_mock = MockServer::start();
        app_server.setup_mocks_on(&registry_mock);
        sig_server.setup_mocks_on(&registry_mock);

        let client = create_test_client();
        let image_ref =
            Reference::from_str(&format!("{}/my-app:v1", registry_mock.address())).unwrap();

        let result = client.verify(&image_ref, &[trusted_kp.public_key()]).await;

        assert!(result.is_err(), "Verification should fail with wrong key");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Verification failed"),
            "Unexpected error: {}",
            err_msg
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_verify_fails_if_signature_missing() {
        let trusted_kp = TestKeyPair::new(0);

        let app_server = FakeOciServer::new("my-app", "v1")
            .with_layer(b"binary", "application/vnd.oci.image.layer.v1.tar+gzip");

        let registry_mock = MockServer::start();
        app_server.setup_mocks_on(&registry_mock);

        let client = create_test_client();
        let image_ref =
            Reference::from_str(&format!("{}/my-app:v1", registry_mock.address())).unwrap();

        let result = client.verify(&image_ref, &[trusted_kp.public_key()]).await;

        assert!(result.is_err());
    }

    pub struct FakeOciServer {
        pub server: Option<MockServer>,
        pub repo: String,
        pub tag: String,
        pub layers: Vec<(String, Vec<u8>)>,
        pub manifest: OciImageManifest,
    }

    impl FakeOciServer {
        pub fn new(repo: &str, tag: &str) -> Self {
            Self {
                server: None,
                repo: repo.to_string(),
                tag: tag.to_string(),
                layers: Vec::new(),
                manifest: OciImageManifest::default(),
            }
        }

        pub fn build(mut self) -> Self {
            let server = MockServer::start();
            self.setup_mocks_on(&server);
            self.server = Some(server);
            self
        }

        pub fn with_layer(mut self, content: &[u8], media_type: &str) -> Self {
            let digest_hash = digest(&SHA256, content);
            let digest_str = format!("sha256:{}", hex_bytes(digest_hash.as_ref()));
            self.layers.push((digest_str.clone(), content.to_vec()));

            let layer_descriptor = OciDescriptor {
                media_type: media_type.to_string(),
                digest: digest_str,
                size: content.len() as i64,
                ..Default::default()
            };
            self.manifest.layers.push(layer_descriptor);
            self
        }

        pub fn with_cosign_layer(mut self, content: &[u8], signature: &str) -> Self {
            let digest_hash = digest(&SHA256, content);
            let digest_str = format!("sha256:{}", hex_bytes(digest_hash.as_ref()));

            self.layers.push((digest_str.clone(), content.to_vec()));

            let mut annotations = BTreeMap::new();
            annotations.insert(
                "dev.cosignproject.cosign/signature".to_string(),
                signature.to_string(),
            );

            let layer_descriptor = OciDescriptor {
                media_type: "application/vnd.dev.cosign.simplesigning.v1+json".to_string(),
                digest: digest_str,
                size: content.len() as i64,
                annotations: Some(annotations),
                ..Default::default()
            };
            self.manifest.layers.push(layer_descriptor);
            self
        }

        pub fn setup_mocks_on(&self, server: &MockServer) {
            let manifest_bytes = serde_json::to_vec(&self.manifest).unwrap();
            server.mock(|when, then| {
                when.method(GET)
                    .path(format!("/v2/{}/manifests/{}", self.repo, self.tag));
                then.status(200)
                    .header("Content-Type", "application/vnd.oci.image.manifest.v1+json")
                    .body(manifest_bytes);
            });
            for (digest, content) in &self.layers {
                let content_clone = content.clone();
                let digest_clone = digest.clone();
                let repo_clone = self.repo.clone();
                server.mock(move |when, then| {
                    when.method(GET)
                        .path(format!("/v2/{}/blobs/{}", repo_clone, digest_clone));
                    then.status(200).body(content_clone);
                });
            }
        }

        pub fn with_artifact_type(mut self, artifact_type: &str) -> Self {
            self.manifest.artifact_type = Some(artifact_type.to_string());
            self
        }

        pub fn reference(&self) -> Reference {
            let addr = self.server.as_ref().expect("Call build() first").address();
            Reference::from_str(&format!("{}/{}:{}", addr, self.repo, self.tag)).unwrap()
        }
    }

    fn hex_bytes(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
