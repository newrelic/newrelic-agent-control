use super::{Client, OciClientError};
use crate::{
    http::{
        client::HttpClient,
        config::{HttpConfig, ProxyConfig},
    },
    signature::{public_key::PublicKey, public_key_fetcher::PublicKeyFetcher},
};
use base64::Engine;
use oci_client::{Reference, manifest::OciDescriptor};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};
use tracing::debug;

const DEFAULT_PUBLIC_KEY_FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Represents the signature data included in an OCI layer.
#[derive(Debug, Clone)]
struct SignatureData {
    /// The message signed (raw data of the signature payload)
    pub message: Vec<u8>,
    /// The decoded signature.
    pub signature: Vec<u8>,
}

/// Represents the JSON payload of a Cosign signature.
///
/// This structure follows the "Simple Signing" format used by Sigstore/Cosign to store
/// claims about an image. It corresponds to the payload that is signed by the private key.
///
/// For more details, see the [Cosign Signature Specification](https://github.com/sigstore/cosign/blob/main/specs/SIGNATURE_SPEC.md#payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SimpleSigning {
    pub critical: Critical,
    pub optional: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Critical {
    pub identity: Identity,
    pub image: Image,
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Identity {
    #[serde(rename = "docker-reference")]
    pub docker_reference: String, // Unused for verification, only `docker-manifest-digest` is used. Check Cosign specs for details.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Image {
    #[serde(rename = "docker-manifest-digest")]
    pub docker_manifest_digest: String,
}

impl Client {
    /// Helper to build the [PublicKeyFetcher] corresponding to the client.
    pub(super) fn try_build_public_key_fetcher(
        proxy_config: ProxyConfig,
    ) -> Result<PublicKeyFetcher, OciClientError> {
        let http_config = HttpConfig::new(
            DEFAULT_PUBLIC_KEY_FETCH_TIMEOUT,
            DEFAULT_PUBLIC_KEY_FETCH_TIMEOUT,
            proxy_config,
        );
        let http_client = HttpClient::new(http_config)
            .map_err(|err| OciClientError::Build(format!("failure building http-client: {err}")))?;

        Ok(PublicKeyFetcher::new(http_client))
    }

    /// Helper to verify the signature (as described in [super::Client::verify_signature]) using the provided
    /// public keys.
    pub(super) async fn verify_signature_with_public_keys(
        &self,
        reference: &Reference,
        public_keys: &[PublicKey],
    ) -> Result<Reference, OciClientError> {
        // Resolve manifest digest
        let digest = match reference.digest() {
            Some(digest) => {
                debug!(%digest, "Manifest digest was already informed");
                digest.to_string()
            }
            None => {
                // We cannot use `pull_image_manifest` because, in multi-platform artifacts, we need to obtain the
                // index manifest and `pull_image_manifest` would resolve such index and return the artifact's manifest
                // instead.
                let digest = self
                    .client
                    .fetch_manifest_digest(reference, &self.auth)
                    .await
                    .map_err(|err| {
                        OciClientError::Verify(format!("could not fetch manifest: {err}"))
                    })?;
                debug!(%digest, "Manifest digest resolved");
                digest
            }
        };

        // Calculate signature location
        let signature_ref = triangulate(reference, &digest);
        debug!("Looking for signatures at: {}", signature_ref.whole());

        // Get the corresponding manifest
        let (signature_manifest, _) = self
            .client
            .pull_image_manifest(&signature_ref, &self.auth)
            .await
            .map_err(|err| {
                OciClientError::Verify(format!("could not fetch signature manifest: {err}"))
            })?;

        // Try to validate signature for each valid signature in the manifest's layers
        for layer in signature_manifest.layers {
            let Some(signature_data) = self
                .try_get_signature_data_from_layer(layer, &signature_ref, &digest)
                .await?
            else {
                continue;
            };

            for key in public_keys {
                match key.verify_signature(&signature_data.message, &signature_data.signature) {
                    Ok(()) => {
                        // Valid signature found, return reference with digest
                        return Ok(Reference::with_digest(
                            reference.registry().to_string(),
                            reference.repository().to_string(),
                            digest,
                        ));
                    }
                    Err(err) => {
                        debug!(
                            key_id = key.key_id(),
                            "Signature verification failed: {err}"
                        );
                    }
                }
            }
        }
        Err(OciClientError::Verify(format!(
            "verification failed. Checked with {} public keys, but no valid signature found for reference '{}', digest ({})",
            public_keys.len(),
            reference.whole(),
            digest
        )))
    }

    /// Gets the [SignatureData] from the provided layer, reference and digest if all conditions are met:
    /// * The layer's media_type matches the expected
    /// * The signature is informed in the corresponding annotation in base64
    /// * Contains the signature message as [SimpleSigning]
    /// * The digest is informed in the corresponding field of the signature message
    ///
    /// Returns:
    /// * An error if there is a failure downloading the signature blob
    /// * `Ok(Some(SignatureData))` if some signature data is found in the layer
    /// * `Ok(None)` if some conditions are not met (there is no error but no valid signature is found)
    async fn try_get_signature_data_from_layer(
        &self,
        layer: OciDescriptor,
        reference: &Reference,
        digest: &str,
    ) -> Result<Option<SignatureData>, OciClientError> {
        if layer.media_type != "application/vnd.dev.cosign.simplesigning.v1+json" {
            debug!("Layer with unexpected media_type, skipping");
            return Ok(None);
        }

        let Some(signature) = layer
            .annotations
            .as_ref()
            .and_then(|a| a.get("dev.cosignproject.cosign/signature"))
            .cloned()
        else {
            debug!("Layer missing signature annotation, skipping");
            return Ok(None);
        };

        let mut raw_data = Vec::new();
        self.client
            .pull_blob(reference, &layer, &mut raw_data)
            .await
            .map_err(|err| {
                OciClientError::Verify(format!("failure fetching signature layer: {err}"))
            })?;

        let signing_data = match serde_json::from_slice::<SimpleSigning>(&raw_data) {
            Ok(simple_signing) => simple_signing,
            Err(err) => {
                debug!("Failed to parse signature layer JSON. Skipping: {err}");
                return Ok(None);
            }
        };

        if signing_data.critical.image.docker_manifest_digest != digest {
            debug!(
                claims = signing_data.critical.image.docker_manifest_digest,
                expected = digest,
                "Signature layer skipped: digest mismatch"
            );
            return Ok(None);
        }

        let signature_bytes = match base64::engine::general_purpose::STANDARD.decode(signature) {
            Ok(b) => b,
            Err(err) => {
                debug!("Skipping layer with invalid base64 signature: {err}");
                return Ok(None);
            }
        };

        Ok(Some(SignatureData {
            message: raw_data,
            signature: signature_bytes,
        }))
    }
}

/// Deterministically derives the Cosign signature reference from a target reference.
///
/// Cosign stores signatures as separate artifacts in the same repository. This function
/// constructs the signature reference by taking the original image's registry and
/// repository, and generating a tag based on the provided `digest` (replacing `:`
/// with `-` and appending `.sig`).
///
/// # Example
///
/// * **Input Repo**: `registry.io/my-app`
/// * **Input Digest**: `sha256:9f86...`
/// * **Output Reference**: `registry.io/my-app:sha256-9f86....sig`
pub fn triangulate(reference: &Reference, digest: &str) -> Reference {
    let signature_tag = format!("{}.sig", digest.replace(':', "-"));

    Reference::with_tag(
        reference.registry().to_string(),
        reference.repository().to_string(),
        signature_tag,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::config::ProxyConfig;
    use crate::signature::public_key::tests::TestKeyPair;
    use crate::{agent_control::run::runtime::tests::tokio_runtime, oci::tests::FakeOciServer};
    use assert_matches::assert_matches;
    use aws_lc_rs::digest::{SHA256, digest};
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use httpmock::{Method::GET, MockServer};
    use oci_client::{
        Reference,
        client::{ClientConfig, ClientProtocol},
        manifest::OciDescriptor,
    };
    use rstest::rstest;
    use std::collections::BTreeMap;
    use std::str::FromStr;

    /// Layers are skipped (Ok(None)) before any blob fetch when the media_type or annotation
    /// conditions are not met.
    #[rstest]
    #[case::wrong_media_type("application/vnd.oci.image.layer.v1.tar+gzip", None)]
    #[case::no_annotations("application/vnd.dev.cosign.simplesigning.v1+json", None)]
    #[case::annotation_without_cosign_key(
        "application/vnd.dev.cosign.simplesigning.v1+json",
        Some("some.other.annotation")
    )]
    fn test_signature_data_from_layer_skipped_before_fetch(
        #[case] media_type: &str,
        #[case] annotation_key: Option<&str>,
    ) {
        let client = create_test_client();
        let reference = Reference::from_str("localhost:1234/repo:tag").unwrap();
        let annotations = annotation_key.map(|k| {
            let mut m = BTreeMap::new();
            m.insert(k.to_string(), "value".to_string());
            m
        });
        let layer = OciDescriptor {
            media_type: media_type.to_string(),
            annotations,
            ..Default::default()
        };

        let result = tokio_runtime().block_on(client.try_get_signature_data_from_layer(
            layer,
            &reference,
            "sha256:abc",
        ));

        assert_matches!(result, Ok(None));
    }

    #[test]
    fn test_signature_data_from_layer_fetch_failure_returns_error() {
        let server = MockServer::start();
        let repo = "my-repo";
        let blob_digest = sha256_of(b"some content");
        server.mock(|when, then| {
            when.method(GET)
                .path(format!("/v2/{repo}/blobs/{blob_digest}"));
            then.status(500);
        });

        let layer = cosign_layer(blob_digest, &BASE64_STANDARD.encode(b"sig"));
        let reference =
            Reference::from_str(&format!("{}/{repo}:sha256-abc.sig", server.address())).unwrap();

        let result = tokio_runtime().block_on(
            create_test_client().try_get_signature_data_from_layer(layer, &reference, "sha256:abc"),
        );

        assert_matches!(result, Err(OciClientError::Verify(_)));
    }

    /// Layers are skipped (Ok(None)) when the blob fetch succeeds but the payload or signature
    /// annotation fails validation.
    #[rstest]
    #[case::invalid_json(blob_invalid_json, "aGVsbG8=", "sha256:expected")]
    #[case::digest_mismatch(blob_with_wrong_digest, "aGVsbG8=", "sha256:expected")]
    #[case::invalid_base64_annotation(
        blob_matching_digest,
        "!!!not-valid-base64!!!",
        "sha256:expected"
    )]
    fn test_signature_data_from_layer_skipped_after_fetch(
        #[case] make_blob: fn(&str) -> Vec<u8>,
        #[case] sig_annotation: &str,
        #[case] expected_digest: &str,
    ) {
        let server = MockServer::start();
        let repo = "my-repo";
        let blob_content = make_blob(expected_digest);
        let blob_digest = sha256_of(&blob_content);
        server.mock(|when, then| {
            when.method(GET)
                .path(format!("/v2/{repo}/blobs/{blob_digest}"));
            then.status(200).body(blob_content);
        });

        let layer = cosign_layer(blob_digest, sig_annotation);
        let reference =
            Reference::from_str(&format!("{}/{repo}:sha256-expected.sig", server.address()))
                .unwrap();

        let result =
            tokio_runtime().block_on(create_test_client().try_get_signature_data_from_layer(
                layer,
                &reference,
                expected_digest,
            ));

        assert_matches!(result, Ok(None));
    }

    #[test]
    fn test_valid_layer_returns_signature_data() {
        let server = MockServer::start();
        let repo = "my-repo";
        let expected_digest = "sha256:expected-digest";
        let payload = simple_signing_payload(expected_digest);
        let raw_sig = b"raw-signature-bytes";
        let blob_digest = sha256_of(&payload);
        let payload_clone = payload.clone();
        server.mock(|when, then| {
            when.method(GET)
                .path(format!("/v2/{repo}/blobs/{blob_digest}"));
            then.status(200).body(payload_clone);
        });

        let layer = cosign_layer(blob_digest, &BASE64_STANDARD.encode(raw_sig));
        let reference =
            Reference::from_str(&format!("{}/{repo}:sha256-expected.sig", server.address()))
                .unwrap();

        let sig_data_result =
            tokio_runtime().block_on(create_test_client().try_get_signature_data_from_layer(
                layer,
                &reference,
                expected_digest,
            ));

        assert_matches!(sig_data_result, Ok(Some(s)) => {
            assert_eq!(s.message, payload);
            assert_eq!(s.signature, raw_sig);
        });
    }

    #[rstest]
    #[case::reference_with_tag(|s: &FakeOciServer| s.reference())]
    // When the reference already contains a digest, the function uses it directly
    // without issuing a fetch_manifest_digest HTTP call.
    #[case::reference_with_digest(|s: &FakeOciServer| s.reference().clone_with_digest(s.manifest_digest()))]
    fn test_verify_success(#[case] ref_fn: impl Fn(&FakeOciServer) -> Reference) {
        let kp = TestKeyPair::new(10);
        let mock_server = FakeOciServer::new("my-app", "v1")
            .with_layer(
                b"binary content",
                "application/vnd.oci.image.layer.v1.tar+gzip",
            )
            .with_signature(&kp)
            .build();

        let reference = ref_fn(&mock_server);

        let result = tokio_runtime().block_on(
            create_test_client().verify_signature_with_public_keys(&reference, &[kp.public_key()]),
        );

        let expected_digest = mock_server.manifest_digest();
        let verified_ref = result.expect("verification should succeed");
        assert_eq!(verified_ref.digest(), Some(expected_digest.as_str()));
    }

    #[test]
    fn test_verify_with_empty_public_keys_returns_error() {
        let kp = TestKeyPair::new(20);
        let mock_server = FakeOciServer::new("my-app", "v1")
            .with_layer(
                b"binary content",
                "application/vnd.oci.image.layer.v1.tar+gzip",
            )
            .with_signature(&kp)
            .build();

        let result = tokio_runtime().block_on(
            create_test_client().verify_signature_with_public_keys(&mock_server.reference(), &[]),
        );
        assert_matches!(result, Err(OciClientError::Verify(_)));
    }

    #[test]
    fn test_verify_with_no_signatures_returns_error() {
        let kp = TestKeyPair::new(10);
        let mock_server = FakeOciServer::new("my-app", "v1")
            .with_layer(
                b"binary content",
                "application/vnd.oci.image.layer.v1.tar+gzip",
            )
            .build(); // `with_signature(kp)` is not called, there is no signature

        let base_ref = mock_server.reference();
        let expected_digest = mock_server.manifest_digest();
        let ref_with_digest = Reference::with_digest(
            base_ref.registry().to_string(),
            base_ref.repository().to_string(),
            expected_digest.clone(),
        );

        let result = tokio_runtime().block_on(
            create_test_client()
                .verify_signature_with_public_keys(&ref_with_digest, &[kp.public_key()]),
        );

        assert_matches!(result, Err(OciClientError::Verify(_)));
    }

    #[test]
    fn test_triangulate_generates_correct_sig_tag() {
        let repo = "my-registry.io/my-repo";
        let digest = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let reference = Reference::from_str(&format!("{}:latest", repo)).unwrap();
        let result = triangulate(&reference, digest);
        assert!(result.tag().unwrap().ends_with(".sig"));
    }

    #[test]
    fn test_simple_signing_deserialization() {
        let json_data = r#"{
            "critical": {
                "identity": { "docker-reference": "registry.com/image" },
                "image": { "docker-manifest-digest": "sha256:abcd" },
                "type": "cosign container image signature"
            },
            "optional": {
                "creator": "cosign",
                "timestamp": "123456789"
            }
        }"#;

        let parsed: SimpleSigning = serde_json::from_str(json_data).unwrap();

        assert_eq!(
            parsed.critical.type_field,
            "cosign container image signature"
        );
        assert_eq!(parsed.critical.image.docker_manifest_digest, "sha256:abcd");
        assert_eq!(parsed.optional.unwrap().get("creator").unwrap(), "cosign");
    }

    fn create_test_client() -> crate::oci::Client {
        crate::oci::Client::try_new(
            ClientConfig {
                protocol: ClientProtocol::Http,
                ..Default::default()
            },
            ProxyConfig::default(),
            tokio_runtime(),
        )
        .expect("Failed to create test Client")
    }

    fn sha256_of(data: &[u8]) -> String {
        let d = digest(&SHA256, data);
        let hex: String = d.as_ref().iter().map(|b| format!("{:02x}", b)).collect();
        format!("sha256:{hex}")
    }

    fn simple_signing_payload(image_digest: &str) -> Vec<u8> {
        serde_json::json!({
            "critical": {
                "identity": { "docker-reference": "registry.io/repo" },
                "image": { "docker-manifest-digest": image_digest },
                "type": "cosign container image signature"
            },
            "optional": {}
        })
        .to_string()
        .into_bytes()
    }

    fn blob_invalid_json(_: &str) -> Vec<u8> {
        b"this is not valid json".to_vec()
    }
    fn blob_with_wrong_digest(_: &str) -> Vec<u8> {
        simple_signing_payload("sha256:wrong-digest")
    }
    fn blob_matching_digest(digest: &str) -> Vec<u8> {
        simple_signing_payload(digest)
    }

    fn cosign_layer(digest: String, sig_annotation: &str) -> OciDescriptor {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "dev.cosignproject.cosign/signature".to_string(),
            sig_annotation.to_string(),
        );
        OciDescriptor {
            media_type: "application/vnd.dev.cosign.simplesigning.v1+json".to_string(),
            digest,
            annotations: Some(annotations),
            ..Default::default()
        }
    }
}
