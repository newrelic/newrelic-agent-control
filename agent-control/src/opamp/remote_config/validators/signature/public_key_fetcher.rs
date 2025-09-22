use http::Request;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::http::client::HttpClient;
use crate::opamp::remote_config::validators::signature::public_key::PublicKey;
use crate::opamp::remote_config::validators::signature::verifier::VerifierFetcher;

#[derive(Error, Debug)]
#[error("fetching public key: {0}")]
pub struct PubKeyFetcherError(String);

/// Fetches a public key from a JWKS remote server.
pub struct PublicKeyFetcher {
    http_client: HttpClient,
    url: Url,
}

impl PublicKeyFetcher {
    pub fn new(http_client: HttpClient, url: Url) -> Self {
        Self { http_client, url }
    }
}

impl VerifierFetcher for PublicKeyFetcher {
    type Error = PubKeyFetcherError;
    type Verifier = PublicKey;
    fn fetch(&self) -> Result<Self::Verifier, Self::Error> {
        let request = Request::builder()
            .method("GET")
            .uri(self.url.as_str())
            .body(Vec::new())
            .map_err(|e| PubKeyFetcherError(format!("building request: {}", e)))?;

        let response = self
            .http_client
            .send(request)
            .map_err(|e| PubKeyFetcherError(format!("sending request: {}", e)))?;

        let payload: PubKeyPayload = serde_json::from_slice(response.body())
            .map_err(|e| PubKeyFetcherError(format!("decoding response: {}", e)))?;

        let Some(latest_key) = payload.keys.first() else {
            return Err(PubKeyFetcherError("missing key data".to_string()));
        };

        PublicKey::try_new(latest_key)
            .map_err(|e| PubKeyFetcherError(format!("building verifier: {}", e)))
    }
}

/// Represents the payload returned by the JWKS endpoint.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PubKeyPayload {
    pub keys: Vec<KeyData>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KeyData {
    pub kty: String,
    pub alg: Option<String>,
    pub r#use: String,
    pub kid: String,
    pub x: String,
    pub crv: String,
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::http::config::HttpConfig;
    use base64::Engine;
    use base64::prelude::{BASE64_STANDARD, BASE64_URL_SAFE_NO_PAD};
    use httpmock::prelude::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use serde_json::json;

    pub struct FakePubKeyServer {
        _server_guard: MockServer,
        key_pair: Ed25519KeyPair,
        pub url: Url,
        pub key_id: String,
    }

    impl FakePubKeyServer {
        pub fn new() -> Self {
            let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
            let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8_bytes.as_ref()).unwrap();
            let public_key = BASE64_URL_SAFE_NO_PAD.encode(key_pair.public_key());
            let key_id = "my-signing-key-test/1".to_string();

            let server = MockServer::start();
            server.mock(|when, then| {
                when.method(GET).path("/pub");
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(json!({
                        "keys": [
                            {
                            "kty": "OKP",
                            "alg": null,
                            "use": "sig",
                            "kid": key_id,
                            "n": null,
                            "e": null,
                            "x": public_key,
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
                            "x": "fake discarded by current implementations",
                            "y": null,
                            "crv": "Ed25519"
                            }
                        ]
                    }));
            });
            FakePubKeyServer {
                url: Url::parse(server.url("/pub").as_str()).unwrap(),
                _server_guard: server,
                key_id,
                key_pair,
            }
        }
        pub fn sign(&self, msg: &[u8]) -> String {
            // Actual implementation from FC side signs the Base64 representation of the SHA256 digest
            // of the message (i.e. the remote configs). Hence, to verify the signature, we need to
            // compute the SHA256 digest of the message, then Base64 encode it, and finally verify
            // the signature against that.
            let digest = ring::digest::digest(&ring::digest::SHA256, msg);
            let msg = BASE64_STANDARD.encode(digest);
            BASE64_STANDARD.encode(self.key_pair.sign(msg.as_bytes()).as_ref())
        }
    }

    #[test]
    fn fetch() {
        let server = FakePubKeyServer::new();

        let fetcher = PublicKeyFetcher {
            http_client: HttpClient::new(HttpConfig::default()).unwrap(),
            url: server.url,
        };

        let public_key = fetcher.fetch().unwrap();
        assert_eq!(public_key.key_id, server.key_id)
    }
}
