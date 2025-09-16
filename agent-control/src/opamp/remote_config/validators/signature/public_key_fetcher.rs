use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PubKeyPayload {
    pub keys: Vec<KeyData>,
}

#[derive(Error, Debug)]
pub enum PubKeyError {
    #[error("parsing PubKey: `{0}`")]
    ParsePubKey(String),

    #[error("validating signature: `{0}`")]
    ValidatingSignature(String),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KeyData {
    pub kty: String,
    pub alg: Option<String>,
    #[serde(rename = "use")]
    pub use_: String,
    pub kid: String,
    pub x: String,
    pub crv: String,
}

#[cfg(test)]
mod tests {
    use crate::opamp::remote_config::validators::signature::public_key_fetcher::PubKeyPayload;
    use serde_json::json;

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
}
