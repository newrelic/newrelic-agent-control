//! This module provides an [oci_client] wrapper.
//! It provides an unified way to build the client and additional features required by Agent Control's
//! usage of OCI repositories.

use crate::http::config::ProxyConfig;
use base64::Engine;
use oci_client::{
    Reference,
    client::{AsLayerDescriptor, ClientConfig},
    manifest::OciImageManifest,
    secrets::RegistryAuth,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::io::AsyncWrite;
use tracing::log::warn;
use tracing::{debug, info};
use url::Url;

mod error;
mod proxy;

use crate::signature::public_key::{PublicKey, SigningAlgorithm};
use crate::signature::public_key_fetcher::PublicKeyFetcher;
pub use error::OciClientError;

/// [oci_client::Client] wrapper with extended functionality.
/// Besides centralizing common iteration with the [oci_client], this wrapper should easy a potential refactor if
/// the upstream client extends its current approach.
/// Specifically if it:
/// - Allows injecting an http-client (we could leverage [crate::http::client::HttpClient]).
/// - Starts using a common [oci-spec](https://crates.io/crates/oci-spec) for exposed types.
#[derive(Clone)]
pub struct Client {
    client: oci_client::Client,
    auth: RegistryAuth,
    key_fetcher: PublicKeyFetcher,
}

impl Client {
    /// Returns a new client with the provided configuration and [RegistryAuth::Anonymous] authentication.
    pub fn try_new(
        client_config: ClientConfig,
        proxy_config: ProxyConfig,
        key_fetcher: PublicKeyFetcher,
    ) -> Result<Self, OciClientError> {
        let client_config = proxy::setup_proxy(client_config, proxy_config)?;
        Ok(Self {
            client: oci_client::Client::new(client_config),
            auth: RegistryAuth::Anonymous,
            key_fetcher,
        })
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

    /// High-level method to ensure an image is signed before using it.
    pub async fn verify(
        &self,
        reference: &Reference,
        url: &Url,
    ) -> Result<Reference, OciClientError> {
        debug!("Fetching public keys from {}", url);

        let fetcher = self.key_fetcher.clone();
        let url = url.clone();

        // Fetches the latest trusted public key from the JWKS endpoint.
        let trusted_key = tokio::task::spawn_blocking(move || fetcher.fetch_latest_key(&url))
            .await
            .map_err(|e| OciClientError::KeyFetch(format!("Task join error: {}", e)))?
            .map_err(|e| OciClientError::KeyFetch(e.to_string()))?;

        // Resolves the target image digest
        let (_, digest_str) = self.pull_image_manifest(reference).await?;
        debug!("Image resolved to digest: {}", digest_str);

        // Locates the signature image tag (Triangulation)
        let signature_ref = self.triangulate(reference, Some(&digest_str))?;
        debug!("Looking for signatures at: {}", signature_ref.whole());

        // Downloads and verifies the signature layers against the trusted key
        let layers = self.trusted_signature_layers(&signature_ref).await?;

        if layers.is_empty() {
            return Err(OciClientError::Verify(format!(
                "No signature layers found for image {}",
                reference.whole()
            )));
        }

        self.verify_signatures(&layers, &digest_str, &trusted_key)?;

        let pinned_ref_str = format!(
            "{}/{}@{}",
            reference.registry(),
            reference.repository(),
            digest_str
        );

        Reference::try_from(pinned_ref_str)
            .map_err(|e| OciClientError::InvalidReference(e.to_string()))
    }

    /// Iterates through candidate signature layers and attempts to verify them.
    /// Returns Ok(()) if at least one valid signature matches the expected image digest.
    fn verify_signatures(
        &self,
        layers: &[SignatureLayer],
        expected_image_digest: &str,
        trusted_key: &PublicKey,
    ) -> Result<(), OciClientError> {
        let mut checked_count = 0;

        for layer in layers {
            // Ensure the signature actually claims to sign THIS image digest.
            if layer.simple_signing.critical.image.docker_reference != expected_image_digest {
                debug!(
                    "Signature skipped: digest mismatch (claims {}, expected {})",
                    layer.simple_signing.critical.image.docker_reference, expected_image_digest
                );
                continue;
            }

            let signature_bytes =
                match base64::engine::general_purpose::STANDARD.decode(&layer.signature) {
                    Ok(b) => b,
                    Err(e) => {
                        warn!("Skipping layer with invalid base64 signature: {}", e);
                        continue;
                    }
                };

            checked_count += 1;

            match trusted_key.verify_signature(
                &SigningAlgorithm::ED25519,
                &layer.raw_data,
                &signature_bytes,
            ) {
                Ok(_) => {
                    info!("Valid signature found and verified via OCI!");
                    return Ok(());
                }
                Err(e) => {
                    debug!("Signature verification failed for layer: {}", e);
                }
            }
        }

        Err(OciClientError::Verify(format!(
            "Verification failed. Checked {} candidates, but no valid signature found for digest {}",
            checked_count, expected_image_digest
        )))
    }

    /// Determines the reference for the signature image based on the target image digest.
    fn triangulate(
        &self,
        reference: &Reference,
        known_digest: Option<&str>,
    ) -> Result<Reference, OciClientError> {
        let digest = known_digest.or(reference.digest()).ok_or_else(|| {
            OciClientError::InvalidReference(
                "Digest required for triangulation not found in reference".into(),
            )
        })?;

        let signature_tag = format!("{}.sig", digest.replace(':', "-"));

        let new_ref_str = format!(
            "{}/{}:{}",
            reference.registry(),
            reference.repository(),
            signature_tag
        );

        Reference::try_from(new_ref_str)
            .map_err(|e| OciClientError::InvalidReference(e.to_string()))
    }

    /// Pulls the signature image manifest and downloads layers containing the Simple Signing payload.
    async fn trusted_signature_layers(
        &self,
        cosign_image_ref: &Reference,
    ) -> Result<Vec<SignatureLayer>, OciClientError> {
        let (manifest, _) = self.pull_image_manifest(cosign_image_ref).await?;
        let mut signature_layers = Vec::new();

        for layer in manifest.layers {
            // We only care about layers that hold a Cosign Simple Signing payload
            if layer.media_type != "application/vnd.dev.cosign.simplesigning.v1+json" {
                continue;
            }

            let Some(signature) = layer
                .annotations
                .as_ref()
                .and_then(|a| a.get("dev.cosignproject.cosign/signature"))
                .cloned()
            else {
                debug!("Layer missing signature annotation, skipping");
                continue;
            };

            let mut raw_data = Vec::new();
            if let Err(e) = self
                .pull_blob(cosign_image_ref, &layer, &mut raw_data)
                .await
            {
                warn!("Failed to pull blob for signature layer: {}", e);
                continue;
            }

            let Ok(simple_signing) = serde_json::from_slice::<SimpleSigning>(&raw_data) else {
                warn!("Failed to parse signature layer JSON. Skipping.");
                continue;
            };

            signature_layers.push(SignatureLayer {
                simple_signing,
                oci_digest: layer.digest,
                raw_data,
                signature,
            });
        }
        Ok(signature_layers)
    }
}

#[derive(Debug, Clone)]
pub struct SignatureLayer {
    pub simple_signing: SimpleSigning,
    pub oci_digest: String,
    pub raw_data: Vec<u8>,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleSigning {
    pub critical: Critical,
    pub optional: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Critical {
    pub identity: ImageIdentity,
    pub image: ImageIdentity,
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageIdentity {
    #[serde(rename = "docker-reference")]
    pub docker_reference: String,
}

#[cfg(test)]
pub mod tests {
    use base64::Engine;
    use httpmock::{Method::GET, MockServer};
    use oci_client::{
        Reference,
        client::ClientConfig,
        manifest::{OciDescriptor, OciImageManifest},
    };
    use ring::digest::{SHA256, digest};
    use std::collections::BTreeMap;
    use std::str::FromStr;
    use tempfile::tempdir;
    use url::Url;

    use crate::oci::{SignatureLayer, SimpleSigning};
    use crate::signature::public_key::tests::TestKeyPair;
    use crate::{http::config::ProxyConfig, oci::Client};

    fn create_test_client() -> Client {
        let http_client =
            crate::http::client::HttpClient::new(crate::http::config::HttpConfig::default())
                .expect("Failed to create test HTTP client");

        let fetcher = crate::signature::public_key_fetcher::PublicKeyFetcher::new(http_client);

        Client::try_new(
            ClientConfig {
                protocol: oci_client::client::ClientProtocol::Http,
                ..Default::default()
            },
            ProxyConfig::default(),
            fetcher,
        )
        .expect("Failed to create test Client")
    }

    #[test]
    fn test_security_replay_attack() {
        let kp = TestKeyPair::new(0);
        let pub_key = kp.public_key();

        let malicious_digest =
            "sha256:1111111111111111111111111111111111111111111111111111111111111111";
        let good_digest = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

        let payload = serde_json::json!({
            "critical": {
                "identity": { "docker-reference": "" },
                "image": { "docker-reference": good_digest },
                "type": "cosign container image signature"
            },
            "optional": {}
        });
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let signature = base64::engine::general_purpose::STANDARD.encode(kp.sign(&payload_bytes));

        let layer = SignatureLayer {
            simple_signing: serde_json::from_slice(&payload_bytes).unwrap(),
            oci_digest: "sha256:layer".to_string(),
            raw_data: payload_bytes,
            signature,
        };

        let client = create_test_client();

        let result = client.verify_signatures(&[layer], malicious_digest, &pub_key);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Verification failed")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_verify_full_flow_integration() {
        let kp = TestKeyPair::new(0);
        let pub_key_jwk = kp.public_key_jwk();

        let jwks_server = MockServer::start();
        jwks_server.mock(|when, then| {
            when.method(GET).path("/jwks.json");
            then.status(200)
                .json_body(serde_json::json!({ "keys": [pub_key_jwk] }));
        });
        let jwks_url = Url::parse(&jwks_server.url("/jwks.json")).unwrap();

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
        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(kp.sign(&payload_bytes));

        let sig_tag = format!("{}.sig", image_manifest_digest_str.replace(':', "-"));

        let sig_server =
            FakeOciServer::new("my-app", &sig_tag).with_cosign_layer(&payload_bytes, &sig_b64);

        let registry_mock = MockServer::start();
        app_server.setup_mocks_on(&registry_mock);
        sig_server.setup_mocks_on(&registry_mock);

        let client = tokio::task::block_in_place(create_test_client);
        let image_ref =
            Reference::from_str(&format!("{}/my-app:v1", registry_mock.address())).unwrap();

        let result = client.verify(&image_ref, &jwks_url).await;

        assert!(result.is_ok(), "Error verifying: {:?}", result.err());
        let verified_ref = result.unwrap();

        assert!(verified_ref.whole().contains(&image_manifest_digest_str));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_client() {
        let server = FakeOciServer::new("repo", "v1.2.3")
            .with_layer(b"some content", "fake-media_type")
            .build();

        let client = tokio::task::block_in_place(create_test_client);
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

    #[test]
    fn test_triangulate_logic() {
        let client = create_test_client();
        let repo_url = "image.io/library/nginx";
        let valid_digest =
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let reference = Reference::from_str(&format!("{}:latest", repo_url)).unwrap();
        let sig_ref = client.triangulate(&reference, Some(valid_digest)).unwrap();
        let expected_tag = format!("{}.sig", valid_digest.replace(':', "-"));
        assert_eq!(sig_ref.tag().unwrap(), expected_tag);
    }

    #[test]
    fn test_cosign_json_deserialization() {
        let json_data = r#"{
            "critical": { "identity": { "docker-reference": "r" }, "image": { "docker-reference": "r" }, "type": "cosign container image signature" },
            "optional": {}
        }"#;
        let parsed: SimpleSigning = serde_json::from_str(json_data).unwrap();
        assert_eq!(
            parsed.critical.type_field,
            "cosign container image signature"
        );
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

        pub fn with_artifact_type(mut self, artifact_type: &str) -> Self {
            self.manifest.artifact_type = Some(artifact_type.to_string());
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

        pub fn build(mut self) -> Self {
            let server = MockServer::start();
            self.setup_mocks_on(&server);
            self.server = Some(server);
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

        pub fn reference(&self) -> Reference {
            let addr = self.server.as_ref().expect("Call build() first").address();
            Reference::from_str(&format!("{}/{}:{}", addr, self.repo, self.tag)).unwrap()
        }
    }

    fn hex_bytes(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
