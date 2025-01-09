use std::collections::HashMap;

use opamp_client::opamp::proto::CustomMessage;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use webpki::SignatureAlgorithm;

pub const SIGNATURE_CUSTOM_CAPABILITY: &str = "com.newrelic.security.configSignature";
pub const SIGNATURE_CUSTOM_MESSAGE_TYPE: &str = "newrelicRemoteConfigSignature";
pub const RSA_PKCS1_2048_8192_SHA256: &str = "RSA_PKCS1_2048_8192_SHA256";
pub const RSA_PKCS1_2048_8192_SHA512: &str = "RSA_PKCS1_2048_8192_SHA512";
pub const ECDSA_P256_SHA256: &str = "ECDSA_P256_SHA256";
pub const ED25519: &str = "ED25519";

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct SignatureData {
    signature: Vec<u8>,
    #[serde(rename = "keyID")]
    key_id: String,
    #[serde(rename = "signingDomain")]
    signing_domain: String,

    checksum: String,
    #[serde(rename = "checksumAlgorithm")]
    checksum_algorithm: String,
    #[serde(rename = "signingAlgorithm")]
    signing_algorithm: String,
    #[serde(rename = "signatureSpecification")]
    signature_specification: String,
}
impl SignatureData {
    pub fn signature(&self) -> &[u8] {
        &self.signature
    }

    pub fn signature_algorithm(&self) -> Result<&SignatureAlgorithm, SignatureError> {
        match self.signing_algorithm.as_str() {
            RSA_PKCS1_2048_8192_SHA256 => Ok(&webpki::RSA_PKCS1_2048_8192_SHA256),
            RSA_PKCS1_2048_8192_SHA512 => Ok(&webpki::RSA_PKCS1_2048_8192_SHA512),
            ECDSA_P256_SHA256 => Ok(&webpki::ECDSA_P256_SHA256),
            ED25519 => Ok(&webpki::ED25519),
            unsupported_algorithm => Err(SignatureError::UnsupportedAlgorithm(
                unsupported_algorithm.to_string(),
            )),
        }
    }
}

// TBD id that links a signature with a config
pub type ConfigID = String;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum SignatureError {
    #[error("invalid config signature capability")]
    InvalidCapability,
    #[error("invalid config signature type")]
    InvalidType,
    #[error("invalid config signature data")]
    InvalidData(String),
    #[error("unsupported signature algorithm")]
    UnsupportedAlgorithm(String),
}

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct Signatures {
    signatures: HashMap<ConfigID, SignatureData>,
}

impl Signatures {
    pub fn is_empty(&self) -> bool {
        self.signatures.is_empty()
    }
    pub fn len(&self) -> usize {
        self.signatures.len()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&ConfigID, &SignatureData)> {
        self.signatures.iter()
    }
}

impl TryFrom<&CustomMessage> for Signatures {
    type Error = SignatureError;

    fn try_from(custom_message: &CustomMessage) -> Result<Self, Self::Error> {
        if custom_message.capability != SIGNATURE_CUSTOM_CAPABILITY {
            return Err(SignatureError::InvalidCapability);
        }
        if custom_message.r#type != SIGNATURE_CUSTOM_MESSAGE_TYPE {
            return Err(SignatureError::InvalidType);
        }
        let signatures: Signatures = serde_json::from_slice(&custom_message.data)
            .map_err(|err| SignatureError::InvalidData(err.to_string()))?;

        // verify signature algorithm is supported
        for (_, signature_data) in signatures.iter() {
            signature_data.signature_algorithm()?;
        }
        Ok(signatures)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::SignatureData;
    use super::Signatures;

    impl Signatures {
        pub fn new_unique(signature: &str, signing_algorithm: &str, key_id: &str) -> Self {
            Self {
                signatures: HashMap::from([(
                    "unique".to_string(),
                    SignatureData::new(signature, signing_algorithm, key_id),
                )]),
            }
        }

        pub fn new_multiple(signatures: impl IntoIterator<Item = SignatureData>) -> Self {
            let signatures: HashMap<String, SignatureData> = signatures
                .into_iter()
                .enumerate()
                .map(|(k, signature)| (format!("{k}"), signature))
                .collect();
            Self { signatures }
        }
    }

    impl SignatureData {
        pub fn new(signature: &str, signing_algorithm: &str, key_id: &str) -> Self {
            Self {
                signature: signature.as_bytes().to_vec(),
                signing_algorithm: signing_algorithm.to_string(),
                key_id: key_id.to_string(),
                ..Default::default()
            }
        }
    }
}
