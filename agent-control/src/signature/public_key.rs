use crate::signature::public_key_fetcher::KeyData;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::UnparsedPublicKey;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

#[derive(Error, Debug)]
pub enum PubKeyError {
    #[error("parsing PubKey: {0}")]
    ParsePubKey(String),

    #[error("validating signature: {0}")]
    ValidatingSignature(String),

    #[error("unsupported algorithm: {0}")]
    UnsupportedAlgorithm(String),
}

/// Supported signing algorithms (public key algorithm + digest algorithm).
/// Currently, the only supported algorithm is Ed25519 which uses SHA-512 as the digest algorithm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "&str")]
pub enum SigningAlgorithm {
    ED25519,
}

const ED25519: &str = "ED25519";

impl TryFrom<&str> for SigningAlgorithm {
    type Error = PubKeyError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.to_uppercase().as_str() {
            ED25519 => Ok(Self::ED25519),
            _unsupported_algorithm => Err(PubKeyError::UnsupportedAlgorithm(s.to_string())),
        }
    }
}

impl AsRef<str> for SigningAlgorithm {
    fn as_ref(&self) -> &str {
        match self {
            SigningAlgorithm::ED25519 => ED25519,
        }
    }
}

#[derive(Debug)]
pub struct PublicKey {
    public_key: UnparsedPublicKey<Vec<u8>>,
    key_id: String,
}

impl PublicKey {
    pub fn key_id(&self) -> &str {
        self.key_id.as_str()
    }

    pub fn verify_signature(
        &self,
        signing_algorithm: &SigningAlgorithm,
        msg: &[u8],
        signature: &[u8],
    ) -> Result<(), PubKeyError> {
        self.public_key.verify(msg, signature).map_err(|_| {
            PubKeyError::ValidatingSignature(format!(
                "with key id {} and algorithm {}",
                self.key_id,
                signing_algorithm.as_ref()
            ))
        })?;

        debug!(%self.key_id, "signature verification succeeded");

        Ok(())
    }
}

// Currently, the only supported key type is Ed25519 which should use the following parameters in the JWKS response.
const SUPPORTED_USE: &str = "sig";
const SUPPORTED_KTY: &str = "OKP";
const SUPPORTED_CRV: &str = ED25519;

impl TryFrom<&KeyData> for PublicKey {
    type Error = PubKeyError;
    fn try_from(data: &KeyData) -> Result<Self, Self::Error> {
        if data.r#use != SUPPORTED_USE {
            return Err(PubKeyError::ParsePubKey("Key use is not 'sig'".to_string()));
        }
        if data.kty != SUPPORTED_KTY {
            return Err(PubKeyError::ParsePubKey(
                "The only supported algorithm is OKP".to_string(),
            ));
        }

        if data.crv.to_uppercase().as_str() != SUPPORTED_CRV {
            return Err(PubKeyError::ParsePubKey(
                "The only supported crv is Ed25519".to_string(),
            ));
        }

        // JWKs make use of the base64url encoding as defined in RFC 4648 [RFC4648]. As allowed by Section 3.2 of the RFC,
        // this specification mandates that base64url encoding when used with JWKs MUST NOT use padding.
        let decoded_key = URL_SAFE_NO_PAD
            .decode(data.x.clone())
            .map_err(|e| PubKeyError::ParsePubKey(e.to_string()))?;

        Ok(PublicKey {
            key_id: data.kid.to_string(),
            public_key: UnparsedPublicKey::new(&ring::signature::ED25519, decoded_key),
        })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::signature::public_key_fetcher::PubKeyPayload;
    use assert_matches::assert_matches;
    use base64::Engine;
    use base64::prelude::{BASE64_STANDARD, BASE64_URL_SAFE_NO_PAD};
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey};
    use serde_json::json;

    pub struct TestKeyPair {
        pub key_pair: Ed25519KeyPair,
        pub index: usize,
    }
    impl TestKeyPair {
        pub fn new(index: usize) -> Self {
            let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
            let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8_bytes.as_ref()).unwrap();
            Self { index, key_pair }
        }
        pub fn public_key(&self) -> PublicKey {
            PublicKey {
                key_id: self.key_id(),
                public_key: UnparsedPublicKey::new(
                    &ring::signature::ED25519,
                    self.key_pair.public_key().as_ref().to_vec(),
                ),
            }
        }
        pub fn public_key_jwk(&self) -> KeyData {
            KeyData {
                kty: "OKP".to_string(),
                alg: None,
                r#use: "sig".to_string(),
                kid: self.key_id(),
                x: BASE64_URL_SAFE_NO_PAD.encode(self.key_pair.public_key().as_ref()),
                crv: "Ed25519".to_string(),
            }
        }
        pub fn key_id(&self) -> String {
            format!("test-key-name/{}", self.index)
        }

        pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
            let signature = self.key_pair.sign(msg);
            signature.as_ref().to_vec()
        }
    }

    #[test]
    fn test_real_example() {
        // This payload was returned by a real staging endpoint
        let signature = BASE64_STANDARD.decode("6l3Jv23SUClwCRzWuFHkZn21laEJiNUu7GXwWK+kDaVCMenLJt9Us+r7LyIqEnfRq/Z5PPJoWaalta6mn/wrDw==").unwrap();
        let message = "my-message-to-be-signed this can be anything";
        let payload = serde_json::from_value::<PubKeyPayload>(json!({"keys":[{"kty":"OKP","alg":null,"use":"sig","kid":"869003544/1","n":null,"e":null,"x":"TpT81pA8z0vYiSK2LLhXzkWYJwrL-kxoNt93lzAb1_Q","y":null,"crv":"Ed25519"}]}))
            .unwrap();

        assert_eq!(payload.keys.len(), 1);
        let first_key = payload.keys.first().unwrap();

        let pub_key = PublicKey::try_from(first_key).unwrap();
        // This is a direct call to the verify function. It isn't concerned with the digest
        // and base64 transformation
        pub_key
            .public_key
            .verify(message.as_bytes(), &signature)
            .unwrap();
    }

    #[test]
    fn test_generating_signature() {
        let test_key_pair = TestKeyPair::new(0);
        let pub_key = test_key_pair.public_key();

        let msg: &[u8] = b"hello, world";
        let signature = test_key_pair.sign(msg);

        pub_key
            .verify_signature(&SigningAlgorithm::ED25519, msg, &signature)
            .unwrap();

        assert_matches!(
            pub_key
                .verify_signature(
                    &SigningAlgorithm::ED25519,
                    "wrong_message".as_bytes(),
                    &signature,
                )
                .unwrap_err(),
            PubKeyError::ValidatingSignature(_)
        );
        assert_matches!(
            pub_key
                .verify_signature(
                    &SigningAlgorithm::ED25519,
                    msg,
                    "wrong_signature".as_bytes(),
                )
                .unwrap_err(),
            PubKeyError::ValidatingSignature(_)
        );
    }
}
