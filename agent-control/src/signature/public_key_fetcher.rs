use crate::http::client::HttpClient;
use crate::signature::public_key::PublicKey;
use http::Request;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::warn;
use url::Url;

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
    /// Fetches the latest public key from the JWKS endpoint. The "latest" is the
    /// key at index 0 of the list.
    pub fn fetch_latest_key(&self) -> Result<PublicKey, PubKeyFetcherError> {
        let payload = self.fetch_jwks()?;

        let Some(latest_key) = payload.keys.first() else {
            return Err(PubKeyFetcherError("missing key data".to_string()));
        };

        PublicKey::try_from(latest_key)
            .map_err(|e| PubKeyFetcherError(format!("building verifier: {}", e)))
    }

    /// Fetches all public keys from the JWKS endpoint. If any keys are invalid, they will be
    /// skipped and a warning will be logged. If no valid keys are found, an error will be returned.
    pub fn fetch(&self) -> Result<Vec<PublicKey>, PubKeyFetcherError> {
        let payload = self.fetch_jwks()?;

        let mut keys = Vec::new();
        let mut errors = Vec::new();

        for key_data in payload.keys.iter() {
            match PublicKey::try_from(key_data) {
                Ok(key) => keys.push(key),
                Err(e) => {
                    let error_msg = format!("invalid key {}: {}", key_data.kid, e);
                    warn!("{}", error_msg);
                    errors.push(error_msg);
                }
            }
        }

        if keys.is_empty() {
            return Err(PubKeyFetcherError(format!(
                "no valid keys found ({} invalid keys: {})",
                errors.len(),
                errors.join(", ")
            )));
        }

        Ok(keys)
    }

    fn fetch_jwks(&self) -> Result<PubKeyPayload, PubKeyFetcherError> {
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

        Ok(payload)
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
    use crate::signature::public_key::tests::TestKeyPair;
    use base64::Engine;
    use base64::prelude::BASE64_STANDARD;
    use httpmock::prelude::*;
    use serde_json::json;

    pub struct FakePubKeyServer {
        _server_guard: JwksMockServer,
        last_key_pair: TestKeyPair,
        pub url: Url,
        pub last_key_id: String,
    }

    impl Default for FakePubKeyServer {
        fn default() -> Self {
            let latest_key_pair = TestKeyPair::new(0);
            let old_key_pair = TestKeyPair::new(1);
            Self::new(vec![latest_key_pair, old_key_pair])
        }
    }

    impl FakePubKeyServer {
        /// The index of the key pairs determines the order they will be returned in the JWKS response, which allows us to control which key is considered the "latest" key by the fetcher.
        pub fn new(key_pairs: Vec<TestKeyPair>) -> Self {
            let mut keys = key_pairs.into_iter();

            let last_key = keys.next().expect("at least one key pair is required");

            let mut remaining_keys = keys
                .map(|kp| serde_json::to_value(kp.public_key_jwk()).unwrap())
                .collect();
            let mut jwks_keys = vec![serde_json::to_value(last_key.public_key_jwk()).unwrap()];
            jwks_keys.append(&mut remaining_keys);

            let server = JwksMockServer::new(jwks_keys);

            FakePubKeyServer {
                url: server.url.clone(),
                _server_guard: server,
                last_key_id: last_key.key_id(),
                last_key_pair: last_key,
            }
        }

        pub fn sign_with_latest(&self, msg: &[u8]) -> String {
            BASE64_STANDARD.encode(self.last_key_pair.sign(msg))
        }
    }

    pub struct JwksMockServer {
        pub _server: MockServer,
        pub url: Url,
    }

    impl JwksMockServer {
        pub fn new(keys: Vec<serde_json::Value>) -> JwksMockServer {
            let server = MockServer::start();

            server.mock(|when, then| {
                when.method(GET).path("/jwks");
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(json!({
                        "keys": keys
                    }));
            });

            let url = server.url("/jwks").parse().unwrap();

            Self {
                _server: server,
                url,
            }
        }
    }

    #[test]
    fn fetch_latest() {
        let key_pair_0 = TestKeyPair::new(0);
        let key_pair_1 = TestKeyPair::new(1);
        let key_pair_2 = TestKeyPair::new(2);

        let expected_key_id = key_pair_0.key_id();

        let server = FakePubKeyServer::new(vec![key_pair_0, key_pair_1, key_pair_2]);

        let fetcher = PublicKeyFetcher {
            http_client: HttpClient::new(HttpConfig::default()).unwrap(),
            url: server.url,
        };

        let public_keys = fetcher.fetch_latest_key().unwrap();

        assert_eq!(
            public_keys.key_id(),
            expected_key_id,
            "Expected the latest key to be fetched based on the order of the keys in the response"
        );
    }

    #[test]
    fn fetch_returns_multiple_keys() {
        let key_pair_0 = TestKeyPair::new(0);
        let key_pair_1 = TestKeyPair::new(1);
        let key_pair_2 = TestKeyPair::new(2);

        let expected_key_ids = vec![
            key_pair_0.key_id(),
            key_pair_1.key_id(),
            key_pair_2.key_id(),
        ];

        let server = FakePubKeyServer::new(vec![key_pair_0, key_pair_1, key_pair_2]);

        let fetcher = PublicKeyFetcher {
            http_client: HttpClient::new(HttpConfig::default()).unwrap(),
            url: server.url,
        };

        let public_keys = fetcher.fetch().unwrap();

        assert_eq!(public_keys.len(), 3, "Expected 3 keys to be fetched");

        let fetched_key_ids: Vec<String> = public_keys
            .iter()
            .map(|key| key.key_id().to_string())
            .collect();

        assert_eq!(
            fetched_key_ids, expected_key_ids,
            "Fetched key IDs should match expected key IDs in order"
        );
    }

    #[test]
    fn fetch_returns_error_when_no_valid_keys() {
        let http_client = HttpClient::new(HttpConfig::default()).unwrap();

        let mock_server = JwksMockServer::new(vec![
            json!({
                "kty": "EC",
                "use": "sig",
                "kid": "invalid-key-1",
                "x": "!!!invalid-base64!!!",
                "crv": "P-256"
            }),
            json!({
                "kty": "EC",
                "use": "sig",
                "kid": "invalid-key-2",
                "x": "@@@also-invalid@@@",
                "crv": "P-256"
            }),
        ]);

        let fetcher = PublicKeyFetcher {
            http_client,
            url: mock_server.url,
        };

        let result = fetcher.fetch();

        let err = result.unwrap_err();
        assert!(
            err.0.contains("no valid keys found"),
            "Error should indicate no valid keys were found, got: {}",
            err.0
        );
        assert!(
            err.0.contains("2 invalid keys"),
            "Error should mention the number of invalid keys, got: {}",
            err.0
        );
    }

    #[test]
    fn fetch_returns_partial_keys_when_some_invalid() {
        let valid_key_pair = TestKeyPair::new(0);
        let valid_key_data = valid_key_pair.public_key_jwk();
        let expected_key_id = valid_key_pair.key_id();

        let http_client = HttpClient::new(HttpConfig::default()).unwrap();

        let mock_server = JwksMockServer::new(vec![
            // valid key
            serde_json::to_value(valid_key_data).unwrap(),
            // invalid keys
            json!({
                "kty": "EC",
                "use": "sig",
                "kid": "invalid-key-1",
                "x": "!!!invalid-base64!!!",
                "crv": "P-256"
            }),
            json!({
                "kty": "EC",
                "use": "sig",
                "kid": "invalid-key-2",
                "x": "@@@also-invalid@@@",
                "crv": "P-256"
            }),
        ]);

        let fetcher = PublicKeyFetcher {
            http_client,
            url: mock_server.url,
        };

        let result = fetcher.fetch();

        assert!(
            result.is_ok(),
            "Should succeed with partial keys when at least one valid key exists"
        );
        let keys = result.unwrap();
        assert_eq!(keys.len(), 1, "Should return only the valid key");
        assert_eq!(
            keys[0].key_id(),
            expected_key_id,
            "Should return the valid key"
        );
    }

    #[test]
    fn fetch_returns_error_when_empty_keys_array() {
        let http_client = HttpClient::new(HttpConfig::default()).unwrap();

        let mock_server = JwksMockServer::new(vec![]);

        let fetcher = PublicKeyFetcher {
            http_client,
            url: mock_server.url,
        };

        let result = fetcher.fetch();

        assert!(
            result.is_err(),
            "Expected an error when keys array is empty"
        );
        let err = result.unwrap_err();
        assert!(
            err.0.contains("no valid keys found"),
            "Error should indicate no valid keys were found, got: {}",
            err.0
        );
    }

    #[test]
    fn fetch_handles_http_error_codes() {
        let server = MockServer::start();
        let http_client = HttpClient::new(HttpConfig::default()).unwrap();

        let test_cases = [
            (400, "Bad Request", "Client Error Body"),
            (500, "Internal Server Error", "Server Error Body"),
            (403, "Forbidden", "Auth Failed"),
        ];

        for &(status_code, reason, body) in &test_cases {
            let path = format!("/error-{}", status_code);
            let mock = server.mock(|when, then| {
                when.method(GET).path(&path);
                then.status(status_code).body(body);
            });

            let fetcher = PublicKeyFetcher {
                http_client: http_client.clone(),
                url: server.url(&path).parse().unwrap(),
            };

            let result = fetcher.fetch_latest_key();

            let err = result.expect_err(&format!("Expected an error for status {}", status_code));
            let err_msg = err.0;

            assert!(
                err_msg.contains("sending request"),
                "Error message should indicate it came from the send step"
            );
            assert!(
                err_msg.contains(&status_code.to_string()),
                "Error message should contain the status code {}",
                status_code
            );
            assert!(
                err_msg.contains(reason),
                "Error message should contain the reason phrase '{}'",
                reason
            );

            mock.assert();
        }
    }
}
