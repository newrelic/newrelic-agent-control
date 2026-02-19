use super::{Client, OciClientError};
use crate::{
    http::{
        client::HttpClient,
        config::{HttpConfig, ProxyConfig},
    },
    signature::{public_key::PublicKey, public_key_fetcher::PublicKeyFetcher},
};
use base64::Engine;
use oci_client::Reference;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};
use tracing::debug;

const DEFAULT_PUBLIC_KEY_FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Internal helper struct that groups the raw signature data downloaded from the registry
/// with its parsed representation.
#[derive(Debug, Clone)]
pub struct SignatureLayer {
    pub simple_signing: SimpleSigning,
    /// The digest of the OCI layer containing this signature.
    pub oci_digest: String,
    /// The raw bytes of the payload (used for cryptographic verification).
    pub raw_data: Vec<u8>,
    /// The base64 encoded signature.
    pub signature: String,
}

/// Represents the JSON payload of a Cosign signature.
///
/// This structure follows the "Simple Signing" format used by Sigstore/Cosign to store
/// claims about an image. It corresponds to the payload that is signed by the private key.
///
/// For more details, see the [Cosign Signature Specification](https://github.com/sigstore/cosign/blob/main/specs/SIGNATURE_SPEC.md#payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleSigning {
    pub critical: Critical,
    pub optional: Option<BTreeMap<String, String>>,
}

/// The `critical` section of the payload.
///
/// According to the specification, consumers MUST reject the signature if the critical
/// section contains any fields they do not understand. It ensures that the signature
/// is strictly bound to a specific image digest and identity.
///
/// See: [Critical Section Spec](https://github.com/sigstore/cosign/blob/main/specs/SIGNATURE_SPEC.md#critical-header)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Critical {
    pub identity: Identity,
    pub image: Image,
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    #[serde(rename = "docker-reference")]
    pub docker_reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Image {
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

    /// Verifies the `reference` signature through [verify_signatures] trying out each public-key in `public_keys`.
    pub(super) async fn verify_signature_with_public_keys(
        &self,
        reference: &Reference,
        public_keys: &[PublicKey],
    ) -> Result<Reference, OciClientError> {
        // Resolve image digest
        let digest = match reference.digest() {
            Some(digest) => {
                debug!(%digest, "Artifact digest was already informed");
                digest.to_string()
            }
            None => {
                let (_, digest) = self
                    .client
                    .pull_image_manifest(reference, &self.auth)
                    .await
                    .map_err(|err| {
                        OciClientError::Verify(format!("could not fetch manifest: {err}"))
                    })?;
                debug!(%digest, "Artifact digest resolved");
                digest
            }
        };

        // Calculate signature location
        let signature_ref = triangulate(reference, &digest);
        debug!("Looking for signatures at: {}", signature_ref.whole());

        // Download signature layers
        let layers = self.fetch_trusted_signature_layers(&signature_ref).await?;

        if layers.is_empty() {
            return Err(OciClientError::Verify(format!(
                "No signature layers found for artifact {}",
                reference.whole()
            )));
        }

        // Verify cryptography (External logic)
        verify_signatures(&layers, &digest, public_keys)?;

        Ok(Reference::with_digest(
            reference.registry().to_string(),
            reference.repository().to_string(),
            digest,
        ))
    }

    /// Fetches and parses candidate signature layers from the triangulated signature image.
    ///
    /// This function pulls the image manifest and iterates through its layers, filtering for
    /// those that match the Cosign `SimpleSigning` media type and contain the required
    /// signature annotation. Valid layers are downloaded and deserialized for verification.
    pub(super) async fn fetch_trusted_signature_layers(
        &self,
        cosign_image_ref: &Reference,
    ) -> Result<Vec<SignatureLayer>, OciClientError> {
        let (manifest, _) = self
            .client
            .pull_image_manifest(cosign_image_ref, &self.auth)
            .await
            .map_err(|err| {
                OciClientError::Verify(format!("could not fetch cosign_image_ref manifest: {err}"))
            })?;
        let mut signature_layers = Vec::new();

        for layer in manifest.layers {
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
                .client
                .pull_blob(cosign_image_ref, &layer, &mut raw_data)
                .await
            {
                debug!("Failed to pull blob for signature layer: {}", e);
                continue;
            }

            let simple_signing = match serde_json::from_slice::<SimpleSigning>(&raw_data) {
                Ok(simple_signing) => simple_signing,
                Err(err) => {
                    debug!("Failed to parse signature layer JSON. Skipping: {err}");
                    continue;
                }
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

/// Iterates through candidate signature layers and attempts to cryptographically verify them.
///
/// This function performs two critical checks for each candidate layer:
/// 1. **Claim Verification:** Ensures the `docker-reference` inside the Simple Signing payload
///    matches the `expected_image_digest`. This prevents replay attacks where a valid signature
///    for one image is maliciously attached to another.
/// 2. **Signature Verification:** Decodes the base64 signature from the layer annotation and
///    attempts to verify it against the provided `trusted_keys` using the Ed25519 algorithm.
///
/// Returns `Ok(())` as soon as a valid signature matching a trusted key is found.
pub fn verify_signatures(
    layers: &[SignatureLayer],
    expected_image_digest: &str,
    trusted_keys: &[PublicKey],
) -> Result<(), OciClientError> {
    let mut checked_count = 0;

    for layer in layers {
        if layer.simple_signing.critical.image.docker_manifest_digest != expected_image_digest {
            debug!(
                claims = layer.simple_signing.critical.image.docker_manifest_digest,
                expected = expected_image_digest,
                "Signature skipped: digest mismatch"
            );
            continue;
        }

        let signature_bytes =
            match base64::engine::general_purpose::STANDARD.decode(&layer.signature) {
                Ok(b) => b,
                Err(e) => {
                    debug!("Skipping layer with invalid base64 signature: {}", e);
                    continue;
                }
            };

        checked_count += 1;

        for key in trusted_keys {
            match key.verify_signature(&layer.raw_data, &signature_bytes) {
                Ok(_) => {
                    debug!(
                        layer = layer.oci_digest,
                        key = key.key_id(),
                        "Signature successfully verified"
                    );
                    return Ok(());
                }
                Err(e) => {
                    debug!(key_id = key.key_id(), "Verification failed against {e}");
                }
            }
        }
    }

    Err(OciClientError::Verify(format!(
        "verification failed. Checked with {} public keys, but no valid signature found for digest {}",
        checked_count, expected_image_digest
    )))
}

/// Deterministically derives the Cosign signature reference from a target image digest.
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
    use crate::signature::public_key::tests::TestKeyPair;
    use std::str::FromStr;

    #[test]
    fn test_triangulate_generates_correct_sig_tag() {
        let repo = "my-registry.io/my-repo";
        let digest = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let reference = Reference::from_str(&format!("{}:latest", repo)).unwrap();
        let result = triangulate(&reference, digest);
        assert!(result.tag().unwrap().ends_with(".sig"));
    }

    #[test]
    fn test_verify_signatures_logic_success() {
        let kp = TestKeyPair::new(0);
        let good_digest = "sha256:1111";
        let payload = serde_json::json!({
            "critical": { "identity": { "docker-reference": "" }, "image": { "docker-manifest-digest": good_digest }, "type": "cosign container image signature" },
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
        assert!(verify_signatures(&[layer], good_digest, &[kp.public_key()]).is_ok());
    }

    #[test]
    fn test_verify_signatures_replay_attack_fails() {
        let kp = TestKeyPair::new(0);
        let valid_digest = "sha256:1111";
        let attacker_digest = "sha256:6666";

        let payload = serde_json::json!({
            "critical": {
                "identity": { "docker-reference": "" },
                "image": { "docker-manifest-digest": valid_digest },
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

        let result = verify_signatures(&[layer], attacker_digest, &[kp.public_key()]);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("verification failed"));
        assert!(err_msg.contains("Checked with 0 public keys"));
    }

    #[test]
    fn test_verify_signatures_wrong_key_fails() {
        let kp_signer = TestKeyPair::new(0);
        let kp_verifier = TestKeyPair::new(1);

        let digest = "sha256:1111";

        let payload = serde_json::json!({
            "critical": {
                "identity": { "docker-reference": "" },
                "image": { "docker-manifest-digest": digest },
                "type": "cosign container image signature"
            },
            "optional": {}
        });
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let signature =
            base64::engine::general_purpose::STANDARD.encode(kp_signer.sign(&payload_bytes));

        let layer = SignatureLayer {
            simple_signing: serde_json::from_slice(&payload_bytes).unwrap(),
            oci_digest: "sha256:layer".to_string(),
            raw_data: payload_bytes,
            signature,
        };

        let result = verify_signatures(&[layer], digest, &[kp_verifier.public_key()]);

        assert!(result.is_err());
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
}
