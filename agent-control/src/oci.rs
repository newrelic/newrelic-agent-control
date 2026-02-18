//! This module provides an [oci_client] wrapper.

use std::{path::Path, sync::Arc};

use crate::{http::config::ProxyConfig, signature::public_key_fetcher::PublicKeyFetcher};

use oci_client::{
    Reference,
    client::{AsLayerDescriptor, ClientConfig},
    manifest::OciImageManifest,
    secrets::RegistryAuth,
};
use tokio::runtime::Runtime;
use url::Url;

mod error;
mod proxy;
mod signature_verification;

pub use error::OciClientError;

/// [oci_client::Client] wrapper with extended functionality.
/// It also wraps all _async_ operations using the underlying runtime, all functions will
/// block the current thread until completion.
pub struct Client {
    client: oci_client::Client,
    auth: RegistryAuth,
    runtime: Arc<Runtime>,
    public_key_fetcher: PublicKeyFetcher,
}

impl Client {
    pub fn try_new(
        client_config: ClientConfig,
        proxy_config: ProxyConfig,
        runtime: Arc<Runtime>,
    ) -> Result<Self, OciClientError> {
        let client_config = proxy::setup_proxy(client_config, proxy_config.clone())?;
        let public_key_fetcher = Self::try_build_public_key_fetcher(proxy_config)?;
        Ok(Self {
            client: oci_client::Client::new(client_config),
            auth: RegistryAuth::Anonymous,
            public_key_fetcher,
            runtime,
        })
    }

    /// Wraps [oci_client::Client::pull_image_manifest].
    pub fn pull_image_manifest(
        &self,
        reference: &Reference,
    ) -> Result<(OciImageManifest, String), OciClientError> {
        self.runtime
            .block_on(self.client.pull_image_manifest(reference, &self.auth))
            .map_err(|err| OciClientError::PullManifest(err.into()))
    }

    /// Pulls  the specified blob through [oci_client::Client::pull_blob] and stores it in the specified file path.
    pub fn pull_blob_to_file(
        &self,
        reference: &Reference,
        layer: impl AsLayerDescriptor,
        path: impl AsRef<Path>,
    ) -> Result<(), OciClientError> {
        self.runtime.block_on(async {
            let mut file = tokio::fs::File::create(path).await.map_err(|err| {
                OciClientError::PullBlob(format!("could not create file: {}", err).into())
            })?;

            self.client
                .pull_blob(reference, layer, &mut file)
                .await
                .map_err(|err| OciClientError::PullBlob(err.to_string().into()))?;

            // Ensure all data is flushed to disk before returning
            file.sync_data().await.map_err(|err| {
                OciClientError::PullBlob(format!("failure syncing data to disk: {err}").into())
            })?;

            Ok(())
        })
    }

    /// Obtains public keys from the provided `public_key_url` and performs signature verification of the provided
    /// `reference`. If verification succeeds, it returns the `reference` (identified by digest) that has been
    /// verified.
    pub fn verify_signature(
        &self,
        reference: &Reference,
        public_key_url: &Url,
    ) -> Result<Reference, OciClientError> {
        let public_keys = self
            .public_key_fetcher
            .fetch(public_key_url)
            .map_err(|err| OciClientError::Verify(format!("could not fetch public keys: {err}")))?;
        self.runtime
            .block_on(self.verify_signature_with_public_keys(reference, &public_keys))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use base64::Engine;
    use httpmock::{Method::GET, MockServer};
    use oci_client::manifest::OciDescriptor;
    use ring::digest::{SHA256, digest};
    use std::collections::BTreeMap;
    use std::str::FromStr;
    use tempfile::tempdir;

    use crate::agent_control::run::runtime::tests::tokio_runtime;
    use crate::signature::public_key::tests::TestKeyPair;
    use crate::signature::public_key_fetcher::tests::JwksMockServer;
    use rstest::rstest;

    fn create_test_client() -> Client {
        Client::try_new(
            ClientConfig {
                protocol: oci_client::client::ClientProtocol::Http,
                ..Default::default()
            },
            ProxyConfig::default(),
            tokio_runtime(),
        )
        .expect("Failed to create test Client")
    }

    #[rstest]
    #[case::valid_signer_key(1, Some(0))]
    #[case::wrong_key(1, None)]
    #[case::mixed_keys_with_valid(2, Some(1))]
    fn test_verify_signature(#[case] num_jwks_keys: usize, #[case] signer_position: Option<usize>) {
        // Build the key pairs for the list
        let jwks_key_pairs: Vec<TestKeyPair> = (0..num_jwks_keys).map(TestKeyPair::new).collect();
        // The signer key corresponds to the key position in `signer_position` if any, otherwise
        // it is public key that is not included in the list
        let separate_signer;
        let kp_signer = match signer_position {
            Some(pos) => &jwks_key_pairs[pos],
            None => {
                separate_signer = TestKeyPair::new(num_jwks_keys + 1);
                &separate_signer
            }
        };

        let app_server = FakeOciServer::new("my-app", "v1")
            .with_layer(b"my binary", "application/vnd.oci.image.layer.v1.tar+gzip");

        let manifest_bytes = serde_json::to_vec(&app_server.manifest).unwrap();
        let manifest_digest = digest(&SHA256, &manifest_bytes);
        let image_manifest_digest_str = format!("sha256:{}", hex_bytes(manifest_digest.as_ref()));

        let payload = serde_json::json!({
            "critical": {
                "identity": { "docker-reference": "" },
                "image": { "docker-reference": image_manifest_digest_str },
                "type": "cosign container image signature"
            },
            "optional": {}
        });
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let sig_b64 =
            base64::engine::general_purpose::STANDARD.encode(kp_signer.sign(&payload_bytes));
        let sig_tag = format!("{}.sig", image_manifest_digest_str.replace(':', "-"));

        let sig_server =
            FakeOciServer::new("my-app", &sig_tag).with_cosign_layer(&payload_bytes, &sig_b64);

        let jwks_keys: Vec<serde_json::Value> = jwks_key_pairs
            .iter()
            .map(|kp| serde_json::to_value(kp.public_key_jwk()).unwrap())
            .collect();
        let jwks_server = JwksMockServer::new(jwks_keys);

        let registry_mock = MockServer::start();
        app_server.setup_mocks_on(&registry_mock);
        sig_server.setup_mocks_on(&registry_mock);

        let client = create_test_client();
        let image_ref =
            Reference::from_str(&format!("{}/my-app:v1", registry_mock.address())).unwrap();

        let result = client.verify_signature(&image_ref, &jwks_server.url);

        if signer_position.is_some() {
            let verified_ref = result.expect("verification should succeed");
            assert!(verified_ref.whole().contains(&image_manifest_digest_str));
        } else {
            assert_matches!(result, Err(OciClientError::Verify(_)));
        }
    }

    #[test]
    fn test_verify_fails_if_signature_is_missing() {
        let trusted_kp = TestKeyPair::new(0);

        let app_server = FakeOciServer::new("my-app", "v1")
            .with_layer(b"binary", "application/vnd.oci.image.layer.v1.tar+gzip");

        let jwks_keys = vec![serde_json::to_value(trusted_kp.public_key_jwk()).unwrap()];
        let jwks_server = JwksMockServer::new(jwks_keys);

        let registry_mock = MockServer::start();
        app_server.setup_mocks_on(&registry_mock);

        let client = create_test_client();
        let image_ref =
            Reference::from_str(&format!("{}/my-app:v1", registry_mock.address())).unwrap();

        let result = client.verify_signature(&image_ref, &jwks_server.url);

        assert_matches!(result, Err(OciClientError::Verify(_)));
    }

    #[test]
    fn test_client() {
        let server = FakeOciServer::new("repo", "v1.2.3")
            .with_layer(b"some content", "fake-media_type")
            .build();

        let client = create_test_client();
        let reference = &server.reference();
        let (image_manifest, _) = client.pull_image_manifest(reference).unwrap();
        let layer = &image_manifest.layers[0];
        assert_eq!(layer.media_type, "fake-media_type");

        let tmp = tempdir().unwrap();
        let filepath = tmp.path().join(&layer.digest);
        client
            .pull_blob_to_file(reference, layer, &filepath)
            .expect("writing blob to file should not fail");
        assert_eq!(std::fs::read(filepath).unwrap(), b"some content");
    }

    pub struct FakeOciServer {
        server: Option<MockServer>,
        repo: String,
        tag: String,
        layers: Vec<(String, Vec<u8>)>,
        manifest: OciImageManifest,
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
