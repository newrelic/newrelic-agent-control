//! This module provides an [oci_client] wrapper.

use std::time::Duration;
use std::{path::Path, sync::Arc};

use crate::agent_control::config::OciAuth;
use crate::utils::retry::{BackoffPolicy, retry_with_backoff};
use crate::{http::config::ProxyConfig, signature::public_key_fetcher::PublicKeyFetcher};

use futures::TryStreamExt;
use oci_client::{
    Reference,
    client::{AsLayerDescriptor, ClientConfig},
    manifest::OciImageManifest,
    secrets::RegistryAuth,
};
use tokio::runtime::Runtime;
use tracing::debug;
use url::Url;

pub mod artifact_definitions;
mod error;
mod proxy;
mod signature_verification;

pub use error::OciClientError;

/// Default no-retry policy: a single attempt, no backoff.
fn default_no_retry_policy() -> BackoffPolicy {
    BackoffPolicy {
        max_attempts: 1,
        base_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
        jitter: false,
    }
}

/// [oci_client::Client] wrapper with extended functionality.
/// It also wraps all _async_ operations using the underlying runtime, all functions will
/// block the current thread until completion.
#[derive(Clone)]
pub struct Client {
    client: oci_client::Client,
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
            public_key_fetcher,
            runtime,
        })
    }

    /// Wraps [oci_client::Client::pull_image_manifest].
    pub fn pull_image_manifest(
        &self,
        reference: &Reference,
        auth: &RegistryAuth,
    ) -> Result<(OciImageManifest, String), OciClientError> {
        self.runtime
            .block_on(self.client.pull_image_manifest(reference, auth))
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

    /// Pulls the specified blob into memory and returns its bytes, rejecting any blob larger than
    /// `max_size_bytes`. The returned bytes are verified against the layer's digest by
    /// [oci_client::Client::pull_blob_stream].
    ///
    /// Prefer this over [Self::pull_blob_to_file] for small artifacts whose content is consumed
    /// in-memory and that should not depend on a writable filesystem location.
    pub fn pull_blob(
        &self,
        reference: &Reference,
        layer: impl AsLayerDescriptor,
        max_size_bytes: usize,
    ) -> Result<Vec<u8>, OciClientError> {
        self.runtime.block_on(async {
            let mut stream = self
                .client
                .pull_blob_stream(reference, layer)
                .await
                .map_err(|err| OciClientError::PullBlob(err.to_string().into()))?;

            // Cheap up-front rejection based on the advertised content length. This is
            // attacker-controlled (it may understate the size or be absent), so it is only an
            // optimization; the hard limit below is enforced while reading regardless.
            if let Some(content_length) = stream.content_length
                && content_length > max_size_bytes as u64
            {
                return Err(OciClientError::PullBlob(
                    format!(
                        "blob content length {content_length} exceeds maximum of {max_size_bytes} bytes"
                    )
                    .into(),
                ));
            }

            let mut blob = Vec::new();
            while let Some(chunk) = stream
                .try_next()
                .await
                .map_err(|err| OciClientError::PullBlob(err.to_string().into()))?
            {
                if blob.len() + chunk.len() > max_size_bytes {
                    return Err(OciClientError::PullBlob(
                        format!("blob exceeds maximum of {max_size_bytes} bytes").into(),
                    ));
                }
                blob.extend_from_slice(&chunk);
            }
            Ok(blob)
        })
    }

    /// Verifies the Cosign signature of an OCI artifact.
    ///
    /// This function performs signature verification on the manifest corresponding to the provided `reference`.
    /// If the reference points to an index-manifest (multi-arch image), the signature of the index-manifest itself
    /// is verified (not the platform-specific manifest underneath it).
    ///
    /// Validation skips transparency log verification as it supports verifying artifacts in a privately deployed
    /// infrastructure (same as `cosign verify --private-infrastructure`).
    ///
    /// The expected signature format follows Cosign's specification, :
    /// - Signatures are stored as separate artifacts in the same registry (the signature reference can be derived from
    ///   the provided `reference` through, see [signature_verification::triangulate] for details).
    /// - Each signature is a JSON payload (Simple Signing format) containing a `critical` section with the
    ///   manifest digest of the signed artifact
    /// - The signature itself is base64-encoded in the layer's annotations under `dev.cosignproject.cosign/signature`
    ///
    /// Public keys are fetched from `public_key_url` and verification will be performed using each key in the
    /// corresponding payload. Signature verification succeeds if one of the signatures in the corresponding manifest
    /// (one signature layer) corresponds to one of the public keys. Such verification uses Ed25519 algorithm.
    ///
    /// If verification succeeds, the verified `reference`, **including digest**, is returned.
    ///
    pub fn verify_signature(
        &self,
        reference: &Reference,
        public_key_url: &Url,
        auth: &RegistryAuth,
    ) -> Result<Reference, OciClientError> {
        let public_keys = self
            .public_key_fetcher
            .fetch(public_key_url)
            .map_err(|err| OciClientError::Verify(format!("could not fetch public keys: {err}")))?;
        self.runtime
            .block_on(self.verify_signature_with_public_keys(reference, &public_keys, auth))
    }
}

/// Retrying "verify-then-fetch" orchestration to be shared among different components.
///
/// It owns a [Client] together with the registry authentication and retry policy, so the
/// artifact-specific downloaders only need to build a reference and supply how to materialize the
/// artifact once a verified reference is resolved.
pub struct OciArtifactFetcher {
    client: Client,
    auth: RegistryAuth,
    policy: BackoffPolicy,
}

impl OciArtifactFetcher {
    /// Returns a fetcher with default (no) retries.
    pub fn new(client: Client, auth: Option<OciAuth>) -> Self {
        Self {
            client,
            auth: auth
                .as_ref()
                .map(RegistryAuth::from)
                .unwrap_or(RegistryAuth::Anonymous),
            policy: default_no_retry_policy(),
        }
    }

    /// Returns a new fetcher with the provided retry policy. For a fixed-interval policy, set
    /// `base_delay == max_delay` and `jitter == false` on the [BackoffPolicy].
    pub fn with_retry_policy(self, policy: BackoffPolicy) -> Self {
        Self { policy, ..self }
    }

    /// Resolves the reference (verifying its signature when `public_key_url` is `Some`) and fetches
    /// the artifact from it, retrying the whole operation per the configured policy.
    ///
    /// `fetch_artifact` performs the artifact-specific work (manifest validation, layer selection
    /// and blob retrieval) once a verified reference is resolved. On exhaustion the last error of
    /// the retried operation is returned, wrapped to signal that all attempts were used.
    pub fn fetch<T>(
        &self,
        base_reference: &Reference,
        public_key_url: Option<&Url>,
        fetch_artifact: impl Fn(&Client, &Reference, &RegistryAuth) -> Result<T, OciClientError>,
    ) -> Result<T, OciClientError> {
        retry_with_backoff(&self.policy, || {
            let reference = self.verified_reference(base_reference, public_key_url)?;

            fetch_artifact(&self.client, &reference, &self.auth)
                .inspect_err(|e| debug!("Download '{reference}' failed with error: {e}"))
        })
        .map_err(|e| OciClientError::AttemptsExceeded(e.to_string()))
    }

    /// Returns the [Reference] to download from, verifying its signature when required.
    ///
    /// When `public_key_url` is `Some`, the artifact's signature is verified via
    /// [Client::verify_signature] and the returned reference is digest-pinned (assuring the artifact
    /// downloaded is the one verified). When it is `None`, signature verification is skipped and the
    /// `base` reference is returned unchanged.
    fn verified_reference(
        &self,
        base: &Reference,
        public_key_url: Option<&Url>,
    ) -> Result<Reference, OciClientError> {
        match public_key_url {
            Some(public_key_url) => self
                .client
                .verify_signature(base, public_key_url, &self.auth),
            None => Ok(base.clone()),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use aws_lc_rs::digest::{SHA256, digest};
    use base64::Engine;
    use httpmock::{Method::GET, MockServer};
    use oci_client::manifest::OciDescriptor;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    use crate::signature::public_key::tests::TestKeyPair;
    use crate::signature::public_key_fetcher::tests::JwksMockServer;
    use crate::utils::test_runtime::tokio_runtime;
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
        let kp_signer = match signer_position {
            Some(pos) => &jwks_key_pairs[pos],
            None => &TestKeyPair::new(num_jwks_keys + 1),
        };

        let mock_server = FakeOciServer::new("my-app", "v1")
            .with_layer(b"my binary", "application/vnd.oci.image.layer.v1.tar+gzip")
            .with_signature(kp_signer)
            .build();

        let jwks_keys: Vec<serde_json::Value> = jwks_key_pairs
            .iter()
            .map(|kp| serde_json::to_value(kp.public_key_jwk()).unwrap())
            .collect();
        let jwks_server = JwksMockServer::new(jwks_keys);

        let client = create_test_client();
        let image_ref = mock_server.reference();
        assert!(image_ref.digest().is_none()); // The reference to be verified doesn't have digest

        let result =
            client.verify_signature(&image_ref, &jwks_server.url, &RegistryAuth::Anonymous);

        if signer_position.is_some() {
            let verified_ref = result.expect("verification should succeed");
            assert_eq!(
                verified_ref.digest().unwrap(),
                mock_server.manifest_digest(),
                "The verified reference should inform the corresponding digest"
            );
        } else {
            assert_matches!(result, Err(OciClientError::Verify(_)));
        }
    }

    #[test]
    fn test_verify_fails_if_signature_is_missing() {
        let trusted_kp = TestKeyPair::new(0);

        let mock_server = FakeOciServer::new("my-app", "v1")
            .with_layer(b"binary", "application/vnd.oci.image.layer.v1.tar+gzip")
            .build();

        let jwks_keys = vec![serde_json::to_value(trusted_kp.public_key_jwk()).unwrap()];
        let jwks_server = JwksMockServer::new(jwks_keys);

        let client = create_test_client();
        let image_ref = mock_server.reference();

        let result =
            client.verify_signature(&image_ref, &jwks_server.url, &RegistryAuth::Anonymous);

        assert_matches!(result, Err(OciClientError::Verify(_)));
    }

    #[test]
    fn test_client() {
        let server = FakeOciServer::new("repo", "v1.2.3")
            .with_layer(b"some content", "fake-media_type")
            .build();

        let client = create_test_client();
        let reference = &server.reference();
        let (image_manifest, _) = client
            .pull_image_manifest(reference, &RegistryAuth::Anonymous)
            .unwrap();
        let layer = &image_manifest.layers[0];
        assert_eq!(layer.media_type, "fake-media_type");

        let tmp = tempdir().unwrap();
        let filepath = tmp.path().join(&layer.digest);
        client
            .pull_blob_to_file(reference, layer, &filepath)
            .expect("writing blob to file should not fail");
        assert_eq!(std::fs::read(filepath).unwrap(), b"some content");
    }

    #[test]
    fn test_pull_blob() {
        let server = FakeOciServer::new("repo", "v1.2.3")
            .with_layer(b"some content", "fake-media_type")
            .build();

        let client = create_test_client();
        let reference = &server.reference();
        let (image_manifest, _) = client
            .pull_image_manifest(reference, &RegistryAuth::Anonymous)
            .unwrap();
        let layer = &image_manifest.layers[0];

        let blob = client
            .pull_blob(reference, layer, 1024)
            .expect("pulling blob into memory should not fail");
        assert_eq!(blob, b"some content");
    }

    #[test]
    fn test_pull_blob_exceeds_max_size() {
        let content = vec![b'x'; 100];
        let server = FakeOciServer::new("repo", "v1.2.3")
            .with_layer(&content, "fake-media_type")
            .build();

        let client = create_test_client();
        let reference = &server.reference();
        let (image_manifest, _) = client
            .pull_image_manifest(reference, &RegistryAuth::Anonymous)
            .unwrap();
        let layer = &image_manifest.layers[0];

        let result = client.pull_blob(reference, layer, 10);
        assert_matches!(result, Err(OciClientError::PullBlob(msg)) => {
            assert!(msg.to_string().contains("exceeds maximum"), "{msg}");
        });
    }

    /// Minimal `fetch_artifact` step for fetcher tests: pulls the manifest's first layer into memory.
    fn pull_first_layer(
        client: &Client,
        reference: &Reference,
        auth: &RegistryAuth,
    ) -> Result<Vec<u8>, OciClientError> {
        let (manifest, _) = client.pull_image_manifest(reference, auth)?;
        let layer = manifest
            .layers
            .first()
            .expect("test manifest should have at least one layer");
        client.pull_blob(reference, layer, 10 * 1024 * 1024)
    }

    fn create_fetcher() -> OciArtifactFetcher {
        OciArtifactFetcher::new(create_test_client(), None)
    }

    #[test]
    fn test_fetch_verifies_signature_and_materializes() {
        let key_pair = TestKeyPair::new(0);
        let jwks_server = JwksMockServer::new(vec![
            serde_json::to_value(key_pair.public_key_jwk()).unwrap(),
        ]);
        let server = FakeOciServer::new("repo", "v1")
            .with_layer(b"content", "fake-media_type")
            .with_signature(&key_pair)
            .build();

        let fetcher = create_fetcher();
        let blob = fetcher
            .fetch(
                &server.reference(),
                Some(&jwks_server.url),
                pull_first_layer,
            )
            .unwrap();
        assert_eq!(blob, b"content");
    }

    #[test]
    fn test_fetch_skips_verification_when_no_public_key() {
        let server = FakeOciServer::new("repo", "v1")
            .with_layer(b"content", "fake-media_type")
            .build();

        let fetcher = create_fetcher();
        let blob = fetcher
            .fetch(&server.reference(), None, pull_first_layer)
            .unwrap();
        assert_eq!(blob, b"content");
    }

    #[test]
    fn test_fetch_fails_when_verification_enabled_but_unsigned() {
        let key_pair = TestKeyPair::new(0);
        let jwks_server = JwksMockServer::new(vec![
            serde_json::to_value(key_pair.public_key_jwk()).unwrap(),
        ]);
        let server = FakeOciServer::new("repo", "v1")
            .with_layer(b"content", "fake-media_type")
            .build(); // unsigned

        let fetcher = create_fetcher();
        let err = fetcher
            .fetch(
                &server.reference(),
                Some(&jwks_server.url),
                pull_first_layer,
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("signature verification failed"),
            "{err}"
        );
    }

    #[test]
    fn test_fetch_returns_last_error_on_missing_manifest() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/v2/repo/manifests/v1");
            then.status(404).json_body(serde_json::json!({
                "errors": [{"code": "MANIFEST_UNKNOWN", "message": "manifest unknown"}]
            }));
        });
        let reference = Reference::with_tag(
            server.address().to_string(),
            "repo".to_string(),
            "v1".to_string(),
        );

        let fetcher = create_fetcher();
        let result = fetcher.fetch(&reference, None, pull_first_layer);
        assert_matches!(result, Err(OciClientError::AttemptsExceeded(msg)) => {
            assert!(msg.contains("pulling image manifest"), "{msg}");
        });
    }

    #[test]
    fn test_fetch_detects_content_not_matching_digest() {
        // The manifest served at the pinned digest does not match that digest (MITM).
        let oci_mock =
            FakeOciServer::new("repo", "v1").with_layer(b"some content", "fake-media_type");
        let server = MockServer::start();
        oci_mock.mock_manifest(
            &server,
            &oci_mock.manifest_digest(),
            b"malicious content".to_vec(),
        );
        let reference = Reference::with_digest(
            server.address().to_string(),
            "repo".to_string(),
            oci_mock.manifest_digest(),
        );

        let fetcher = create_fetcher();
        let err = fetcher
            .fetch(&reference, None, pull_first_layer)
            .unwrap_err();
        assert!(err.to_string().contains("Digest error"), "{err}");
    }

    #[test]
    fn test_fetch_pins_digest_against_toctou() {
        const ORIGINAL: &[u8] = b"A";
        const MALICIOUS: &[u8] = b"B";

        let key_pair = TestKeyPair::new(0);
        let jwks_server = JwksMockServer::new(vec![
            serde_json::to_value(key_pair.public_key_jwk()).unwrap(),
        ]);
        let oci_mock_a = FakeOciServer::new("repo", "v1")
            .with_layer(ORIGINAL, "fake-media_type")
            .with_signature(&key_pair);
        let server = MockServer::start();
        oci_mock_a.setup_mocks_on(&server);

        // Verify the signature to obtain a digest-pinned reference.
        let reference = oci_mock_a.reference_on_server(&server);
        let fetcher = create_fetcher();
        let verified_reference = fetcher
            .verified_reference(&reference, Some(&jwks_server.url))
            .expect("signature should verify");

        // Move the tag after verification (TOCTOU attack).
        let oci_mock_b = FakeOciServer::new("repo", "v1").with_layer(MALICIOUS, "fake-media_type");
        server.reset();
        oci_mock_b.setup_mocks_on(&server); // new tag takes precedence
        oci_mock_a.setup_mocks_on(&server); // keep the previous digest and blobs reachable

        // Sanity check: pulling by tag now yields the malicious content.
        let malicious = fetcher.fetch(&reference, None, pull_first_layer).unwrap();
        assert_eq!(malicious, MALICIOUS);

        // The pinned reference still resolves to the originally verified content.
        let original = fetcher
            .fetch(&verified_reference, None, pull_first_layer)
            .unwrap();
        assert_eq!(original, ORIGINAL);
    }

    pub struct FakeOciServer {
        server: Option<MockServer>,
        repo: String,
        tag: String,
        layers: Vec<(String, Vec<u8>)>,
        manifest: OciImageManifest,
        signature: Option<(String, OciImageManifest)>,
    }

    impl FakeOciServer {
        pub fn new(repo: &str, tag: &str) -> Self {
            Self {
                server: None,
                repo: repo.to_string(),
                tag: tag.to_string(),
                signature: None,
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

        pub fn with_signature(mut self, signer: &TestKeyPair) -> Self {
            // Sign the manifest
            let manifest_bytes = serde_json::to_vec(&self.manifest).unwrap();
            let manifest_digest = digest(&SHA256, &manifest_bytes);
            let image_manifest_digest_str =
                format!("sha256:{}", hex_bytes(manifest_digest.as_ref()));
            let payload = serde_json::json!({
                "critical": {
                    "identity": { "docker-reference": "" },
                    "image": { "docker-manifest-digest": image_manifest_digest_str },
                    "type": "cosign container image signature"
                },
                "optional": {}
            });
            let payload_bytes = serde_json::to_vec(&payload).unwrap();
            let sig_b64 =
                base64::engine::general_purpose::STANDARD.encode(signer.sign(&payload_bytes));
            let sig_tag = format!("{}.sig", image_manifest_digest_str.replace(':', "-"));

            // Setup the signature's manifest and the corresponding layer
            let mut signature_manifest = OciImageManifest::default();

            let digest_hash = digest(&SHA256, &payload_bytes);
            let digest_str = format!("sha256:{}", hex_bytes(digest_hash.as_ref()));
            self.layers
                .push((digest_str.clone(), payload_bytes.clone()));

            let mut annotations = BTreeMap::new();
            annotations.insert("dev.cosignproject.cosign/signature".to_string(), sig_b64);

            let layer_descriptor = OciDescriptor {
                media_type: "application/vnd.dev.cosign.simplesigning.v1+json".to_string(),
                digest: digest_str,
                size: payload_bytes.len() as i64,
                annotations: Some(annotations),
                ..Default::default()
            };

            signature_manifest.layers.push(layer_descriptor);

            self.signature = Some((sig_tag, signature_manifest));

            self
        }

        pub fn setup_mocks_on(&self, server: &MockServer) {
            let manifest_bytes = serde_json::to_vec(&self.manifest).unwrap();
            let manifest_digest = self.manifest_digest();
            // Mock manifest by tag
            self.mock_manifest(server, &self.tag, manifest_bytes.clone());
            // Mock manifest by digest
            self.mock_manifest(server, &manifest_digest, manifest_bytes);
            // Mock signature manifest
            if let Some((sig_tag, sig_manifest)) = self.signature.as_ref() {
                let manifest_bytes = serde_json::to_vec(sig_manifest).unwrap();
                self.mock_manifest(server, sig_tag, manifest_bytes);
            }
            // Mock layers
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

        pub fn mock_manifest(&self, server: &MockServer, path: &str, content: Vec<u8>) {
            server.mock(|when, then| {
                when.method(GET)
                    .path(format!("/v2/{}/manifests/{}", self.repo, path));
                then.status(200)
                    .header("Content-Type", "application/vnd.oci.image.manifest.v1+json")
                    .body(content);
            });
        }

        pub fn with_artifact_type(mut self, artifact_type: &str) -> Self {
            self.manifest.artifact_type = Some(artifact_type.to_string());
            self
        }

        pub fn registry(&self) -> String {
            self.server
                .as_ref()
                .expect("Call build() first")
                .address()
                .to_string()
        }

        pub fn reference(&self) -> Reference {
            self.reference_on_server(self.server.as_ref().expect("Call build() first"))
        }

        pub fn reference_on_server(&self, server: &MockServer) -> Reference {
            let addr = server.address().to_string();
            Reference::with_tag(addr, self.repo.clone(), self.tag.clone())
        }

        /// Returns the `digest` for the MockServer's manifest.
        /// Check the [OCI specs](https://github.com/opencontainers/image-spec/blob/6529f89e290d8169adbddf15e43493b9fdd37b62/descriptor.md#L69)
        /// for details.
        /// We don't need JSON canonicalization (which would probably be required in real server implementation) because
        /// we are always getting the same JSON representation of the manifest in the mock-server.
        pub fn manifest_digest(&self) -> String {
            let manifest_bytes = serde_json::to_vec(&self.manifest).unwrap();
            let manifest_digest = digest(&SHA256, &manifest_bytes);
            format!("sha256:{}", hex_bytes(manifest_digest.as_ref()))
        }
    }

    fn hex_bytes(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
