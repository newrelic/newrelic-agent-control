use super::{Client, OciClientError};
use crate::signature::public_key::{PublicKey, SigningAlgorithm};
use base64::Engine;
use oci_client::Reference;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, info};

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

/// Fetches and parses candidate signature layers from the triangulated signature image.
///
/// This function pulls the image manifest and iterates through its layers, filtering for
/// those that match the Cosign `SimpleSigning` media type and contain the required
/// signature annotation. Valid layers are downloaded and deserialized for verification.
pub async fn fetch_trusted_signature_layers(
    client: &Client,
    cosign_image_ref: &Reference,
) -> Result<Vec<SignatureLayer>, OciClientError> {
    let (manifest, _) = client.pull_image_manifest(cosign_image_ref).await?;
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
        if let Err(e) = client
            .pull_blob(cosign_image_ref, &layer, &mut raw_data)
            .await
        {
            debug!("Failed to pull blob for signature layer: {}", e);
            continue;
        }

        let Ok(simple_signing) = serde_json::from_slice::<SimpleSigning>(&raw_data) else {
            debug!("Failed to parse signature layer JSON. Skipping.");
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
                    debug!("Skipping layer with invalid base64 signature: {}", e);
                    continue;
                }
            };

        checked_count += 1;

        for key in trusted_keys {
            match key.verify_signature(
                &SigningAlgorithm::ED25519,
                &layer.raw_data,
                &signature_bytes,
            ) {
                Ok(_) => {
                    info!(
                        "Valid signature found in layer {} and verified via OCI using key {}!",
                        layer.oci_digest,
                        key.key_id()
                    );
                    return Ok(());
                }
                Err(e) => {
                    debug!("Verification failed against key {}: {}", key.key_id(), e);
                }
            }
        }
    }

    Err(OciClientError::Verify(format!(
        "Verification failed. Checked {} candidates, but no valid signature found for digest {}",
        checked_count, expected_image_digest
    )))
}

/// Deterministically derives the Cosign signature reference from the target image digest.
///
/// Cosign stores signatures as separate artifacts in the same repository. The signature tag
/// is generated by sanitizing the image digest (replacing `:` with `-`) and appending `.sig`.
///
/// # Example
///
/// * **Input Repo**: `registry.io/my-app`
/// * **Input Digest**: `sha256:9f86...`
/// * **Output Reference**: `registry.io/my-app:sha256-9f86....sig`
pub fn triangulate(
    reference: &Reference,
    known_digest: Option<&str>,
) -> Result<Reference, OciClientError> {
    let digest = known_digest.or(reference.digest()).ok_or_else(|| {
        OciClientError::InvalidReference(
            "Digest required for triangulation not found in reference".into(),
        )
    })?;

    let new_ref_str = format!(
        "{}/{}:{}.sig",
        reference.registry(),
        reference.repository(),
        digest.replace(':', "-")
    );

    Reference::try_from(new_ref_str).map_err(|e| OciClientError::InvalidReference(e.to_string()))
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
        let result = triangulate(&reference, Some(digest)).unwrap();
        assert!(result.tag().unwrap().ends_with(".sig"));
    }
    #[test]
    fn test_triangulate_extracts_digest_from_pinned_reference() {
        let digest = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let reference = Reference::from_str(&format!("my-repo@{}", digest)).unwrap();

        let result = triangulate(&reference, None).unwrap();

        assert!(result.tag().unwrap().starts_with("sha256-e3b0c442"));
        assert!(result.tag().unwrap().ends_with(".sig"));
    }

    #[test]
    fn test_triangulate_fails_without_digest() {
        let reference = Reference::from_str("my-repo:latest").unwrap();
        let result = triangulate(&reference, None);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Digest required"));
    }

    #[test]
    fn test_verify_signatures_logic_success() {
        let kp = TestKeyPair::new(0);
        let good_digest = "sha256:1111";
        let payload = serde_json::json!({
            "critical": { "identity": { "docker-reference": "" }, "image": { "docker-reference": good_digest }, "type": "cosign container image signature" },
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
                "image": { "docker-reference": valid_digest },
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
        assert!(err_msg.contains("Verification failed"));
        assert!(err_msg.contains("Checked 0 candidates"));
    }

    #[test]
    fn test_verify_signatures_wrong_key_fails() {
        let kp_signer = TestKeyPair::new(0);
        let kp_verifier = TestKeyPair::new(1);

        let digest = "sha256:1111";

        let payload = serde_json::json!({
            "critical": {
                "identity": { "docker-reference": "" },
                "image": { "docker-reference": digest },
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
                "image": { "docker-reference": "sha256:abcd" },
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
        assert_eq!(parsed.critical.image.docker_reference, "sha256:abcd");
        assert_eq!(parsed.optional.unwrap().get("creator").unwrap(), "cosign");
    }
}
