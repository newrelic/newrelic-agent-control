use crate::opamp::remote_config::signature::SigningAlgorithm;
use crate::opamp::remote_config::validators::signature::public_key_fetcher::{
    KeyData, PubKeyError,
};
use crate::opamp::remote_config::validators::signature::verifier::Verifier;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{ED25519, UnparsedPublicKey};

pub struct PublicKey {
    pub public_key: UnparsedPublicKey<Vec<u8>>,
    pub key_id: String,
}

impl PublicKey {
    pub fn try_new(data: &KeyData) -> Result<Self, PubKeyError> {
        if data.use_ != "sig" {
            return Err(PubKeyError::ParsePubKey("Key use is not 'sig'".to_string()));
        }
        if data.kty != "OKP" {
            return Err(PubKeyError::ParsePubKey(
                "The only supported algorithm is Okp".to_string(),
            ));
        }

        if data.crv != "Ed25519" {
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

        self.public_key
            .verify(msg, signature)
            .map_err(|e| PubKeyError::ValidatingSignature(e.to_string()))
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
    use serde_json::json;
    use x509_parser::nom::AsBytes;

    #[test]
    fn test_certificate_validation() {
        let payload = serde_json::from_value::<PubKeyPayload>(json!({"keys":[{"kty":"OKP","alg":null,"use":"sig","kid":"869003544/1","n":null,"e":null,"x":"TpT81pA8z0vYiSK2LLhXzkWYJwrL-kxoNt93lzAb1_Q","y":null,"crv":"Ed25519"}]}))
            .unwrap();

        assert_eq!(payload.keys.len(), 1);
        let first_key = payload.keys.first().unwrap();

        let sign = BASE64_STANDARD.decode("6l3Jv23SUClwCRzWuFHkZn21laEJiNUu7GXwWK+kDaVCMenLJt9Us+r7LyIqEnfRq/Z5PPJoWaalta6mn/wrDw==").unwrap();

        let a = PublicKey::try_new(first_key).unwrap();
        a.verify_signature(
            &ED25519,
            "my-message-to-be-signed this can be anything".as_bytes(),
            sign.as_bytes(),
        )
        .unwrap();

        a.verify_signature(&ED25519, "wrong_message".as_bytes(), sign.as_bytes())
            .unwrap_err();
        a.verify_signature(
            &ED25519,
            "my-message-to-be-signed this can be anything".as_bytes(),
            "wrong_signature".as_bytes(),
        )
        .unwrap_err();
    }
}
