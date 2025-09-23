use crate::opamp::remote_config::signature::SigningAlgorithm;
use crate::opamp::remote_config::validators::signature::public_key_fetcher::KeyData;
use crate::opamp::remote_config::validators::signature::verifier::Verifier;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::{Engine, prelude::BASE64_STANDARD};
use ring::digest;
use ring::signature::{ED25519, UnparsedPublicKey};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PubKeyError {
    #[error("parsing PubKey: {0}")]
    ParsePubKey(String),

    #[error("validating signature: {0}")]
    ValidatingSignature(String),
}

pub struct PublicKey {
    pub public_key: UnparsedPublicKey<Vec<u8>>,
    pub key_id: String,
}

const SUPPORTED_USE: &str = "sig";
const SUPPORTED_KTY: &str = "OKP";
const SUPPORTED_CRV: &str = "Ed25519";

impl PublicKey {
    pub fn try_new(data: &KeyData) -> Result<Self, PubKeyError> {
        if data.r#use != SUPPORTED_USE {
            return Err(PubKeyError::ParsePubKey("Key use is not 'sig'".to_string()));
        }
        if data.kty != SUPPORTED_KTY {
            return Err(PubKeyError::ParsePubKey(
                "The only supported algorithm is Okp".to_string(),
            ));
        }

        if data.crv != SUPPORTED_CRV {
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
            public_key: UnparsedPublicKey::new(&ED25519, decoded_key),
        })
    }
}

impl Verifier for PublicKey {
    type Error = PubKeyError;

    fn verify_signature(
        &self,
        signing_algorithm: &SigningAlgorithm,
        msg: &[u8],
        signature: &[u8],
    ) -> Result<(), Self::Error> {
        if signing_algorithm != &SigningAlgorithm::ED25519 {
            return Err(PubKeyError::ValidatingSignature(
                "The only supported algorithm is Ed25519".to_string(),
            ));
        }

        // Actual implementation from FC side signs the Base64 representation of the SHA256 digest
        // of the message (i.e. the remote configs). Hence, to verify the signature, we need to
        // compute the SHA256 digest of the message, then Base64 encode it, and finally verify
        // the signature against that.
        let msg = digest::digest(&digest::SHA256, msg);
        let msg = BASE64_STANDARD.encode(msg);

        self.public_key
            .verify(msg.as_bytes(), signature)
            .map_err(|_| {
                PubKeyError::ValidatingSignature("signature verification failed".to_string())
            })
    }

    fn key_id(&self) -> &str {
        self.key_id.as_str()
    }
}

#[cfg(test)]
mod tests {
    use crate::opamp::remote_config::signature::SigningAlgorithm::ED25519;
    use crate::opamp::remote_config::validators::signature::public_key::PublicKey;
    use crate::opamp::remote_config::validators::signature::public_key_fetcher::PubKeyPayload;
    use crate::opamp::remote_config::validators::signature::verifier::Verifier;
    use base64::Engine;
    use base64::prelude::BASE64_STANDARD;
    use ring::digest;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey};
    use serde_json::json;
    use x509_parser::nom::AsBytes;

    #[test]
    fn test_real_example() {
        // This payload was returned by a real staging endpoint
        let signature = BASE64_STANDARD.decode("6l3Jv23SUClwCRzWuFHkZn21laEJiNUu7GXwWK+kDaVCMenLJt9Us+r7LyIqEnfRq/Z5PPJoWaalta6mn/wrDw==").unwrap();
        let message = "my-message-to-be-signed this can be anything";
        let payload = serde_json::from_value::<PubKeyPayload>(json!({"keys":[{"kty":"OKP","alg":null,"use":"sig","kid":"869003544/1","n":null,"e":null,"x":"TpT81pA8z0vYiSK2LLhXzkWYJwrL-kxoNt93lzAb1_Q","y":null,"crv":"Ed25519"}]}))
            .unwrap();

        assert_eq!(payload.keys.len(), 1);
        let first_key = payload.keys.first().unwrap();

        let pub_key = PublicKey::try_new(first_key).unwrap();
        // This is a direct call to the verify function. It isn't concerned with the digest
        // and base64 transformation
        pub_key
            .public_key
            .verify(message.as_bytes(), signature.as_bytes())
            .unwrap();
    }

    #[test]
    fn test_generating_signature() {
        let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8_bytes.as_ref()).unwrap();

        const MESSAGE: &[u8] = b"hello, world";
        // Actual implementation from FC side signs the Base64 representation of the SHA256 digest
        // of the message (i.e. the remote configs). Hence, to verify the signature, we need to
        // compute the SHA256 digest of the message, then Base64 encode it, and finally verify
        // the signature against that.
        let digest = digest::digest(&digest::SHA256, MESSAGE);
        let msg = BASE64_STANDARD.encode(digest);

        let signature = key_pair.sign(msg.as_bytes());

        let pub_key = PublicKey {
            key_id: "my-signing-key-test/0".to_string(),
            public_key: UnparsedPublicKey::new(
                &ring::signature::ED25519,
                key_pair.public_key().as_ref().to_vec(),
            ),
        };
        pub_key
            .verify_signature(&ED25519, MESSAGE, signature.as_ref().as_bytes())
            .unwrap();

        pub_key
            .verify_signature(
                &ED25519,
                "wrong_message".as_bytes(),
                signature.as_ref().as_bytes(),
            )
            .unwrap_err();

        pub_key
            .verify_signature(&ED25519, MESSAGE, "wrong_signature".as_bytes())
            .unwrap_err();
    }
}
