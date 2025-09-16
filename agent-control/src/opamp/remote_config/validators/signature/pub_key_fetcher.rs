use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openssl::pkey::{Id, PKey, Public};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use thiserror::Error;
use x509_parser::nom::AsBytes;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PubKeyPayload {
    keys: Vec<KeyData>,
}

#[derive(Error, Debug)]
pub enum PubKeyError {
    #[error("parsing PubKey: `{0}`")]
    ParsePubKey(String),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct KeyData {
    pub kty: String,
    pub alg: Option<String>,
    #[serde(rename = "use")]
    pub use_: String,
    pub kid: String,
    pub n: Option<String>,
    pub e: Option<String>,
    pub x: String,
    pub y: Option<String>,
    pub crv: String,
}

impl KeyData {
    pub fn to_verification_key(&self) -> Result<PKey<Public>, PubKeyError> {
        if self.use_ != "sig" {
            return Err(PubKeyError::ParsePubKey("Key use is not 'sig'".to_string()));
        }
        if self.kty != "OKP" {
            return Err(PubKeyError::ParsePubKey(
                "The only supported algorithm is Okp".to_string(),
            ));
        }

        if self.crv != "Ed25519" {
            return Err(PubKeyError::ParsePubKey(
                "The only supported crv is Ed25519".to_string(),
            ));
        }

        // JWKs make use of the base64url encoding as defined in RFC 4648 [RFC4648]. As allowed by Section 3.2 of the RFC,
        // this specification mandates that base64url encoding when used with JWKs MUST NOT use padding.
        let x = URL_SAFE_NO_PAD.decode(self.x.clone()).unwrap();
        PKey::public_key_from_raw_bytes(x.as_bytes(), Id::ED25519)
            .map_err(|e| PubKeyError::ParsePubKey(e.to_string()))
    }

    pub fn verify_signature(&self, msg: &[u8], signature: &[u8]) -> bool {
        let key = self.to_verification_key().unwrap();
        let mut verifier = openssl::sign::Verifier::new_without_digest(&key).unwrap();
        verifier.verify_oneshot(signature, msg).unwrap()
    }
}

pub trait Verifier {
    type Error: Display;

    fn verify_signature(
        &self,
        algorithm: &webpki::SignatureAlgorithm,
        msg: &[u8],
        signature: &[u8],
    ) -> Result<(), Self::Error>;
}

#[cfg(test)]
mod tests {
    use crate::opamp::remote_config::validators::signature::pub_key_fetcher::PubKeyPayload;
    use base64::Engine;
    use base64::prelude::BASE64_STANDARD;
    use serde_json::json;
    use x509_parser::nom::AsBytes;

    #[test]
    fn test_certificate_key_id() {
        let payload = serde_json::from_value::<PubKeyPayload>(json!({
          "keys": [
            {
              "kty": "OKP",
              "alg": null,
              "use": "sig",
              "kid": "my-signing-key-test/1",
              "n": null,
              "e": null,
              "x": "l5FbLTPxv4TxhYTanUwdxFxoh_X2UYIbQYKyRr-0xnw",
              "y": null,
              "crv": "Ed25519"
            },
            {
              "kty": "OKP",
              "alg": null,
              "use": "sig",
              "kid": "my-signing-key-test/0",
              "n": null,
              "e": null,
              "x": "rA2BPHW1vkVpdX6s8Bp9WKXUZJb7W1oYHJeDFfhxNVw",
              "y": null,
              "crv": "Ed25519"
            }
          ]
        }))
        .unwrap();

        assert_eq!(payload.keys.len(), 2);
        let first_key = payload.keys.first().unwrap();
        assert_eq!(first_key.kid, "my-signing-key-test/1")
    }

    #[test]
    fn test_certificate_key_id_2() {
        let payload = serde_json::from_value::<PubKeyPayload>(json!({"keys":[{"kty":"OKP","alg":null,"use":"sig","kid":"869003544/1","n":null,"e":null,"x":"TpT81pA8z0vYiSK2LLhXzkWYJwrL-kxoNt93lzAb1_Q","y":null,"crv":"Ed25519"}]}))
        .unwrap();

        assert_eq!(payload.keys.len(), 1);
        let first_key = payload.keys.first().unwrap();

        let sign = BASE64_STANDARD.decode("6l3Jv23SUClwCRzWuFHkZn21laEJiNUu7GXwWK+kDaVCMenLJt9Us+r7LyIqEnfRq/Z5PPJoWaalta6mn/wrDw==").unwrap();

        let res = first_key.verify_signature(
            "2_my-message-to-be-signed this can be anything".as_bytes(),
            sign.as_bytes(),
        );
        assert_eq!(res, true)
    }
}
