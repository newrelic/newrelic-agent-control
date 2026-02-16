use base64::{Engine, prelude::BASE64_STANDARD};
use ring::digest;
use std::sync::Mutex;
use thiserror::Error;
use tracing::debug;
use url::Url;

use crate::signature::public_key::PublicKey;
use crate::signature::public_key_fetcher::PublicKeyFetcher;

#[derive(Error, Debug, PartialEq)]
pub enum VerifierStoreError {
    #[error("fetching verifying key: {0}")]
    Fetch(String),
    #[error(
        "signature key ID ({signature_key_id}) does not match the latest available key ID ({stored_key_id})"
    )]
    KeyMismatch {
        signature_key_id: String,
        stored_key_id: String,
    },
    #[error("validating signature: {0}")]
    VerifySignature(String),
    #[error("decoding signature: {0}")]
    DecodingSignature(String),
}

/// VerifierStore provides a way to verify signatures given a key-id.
/// It holds a Verifier and implements the mechanism to refresh it when the key-id changes.
pub struct VerifierStore {
    verifier: Mutex<PublicKey>,
    public_key_url: Url,
    fetcher: PublicKeyFetcher,
}

impl VerifierStore {
    pub fn try_new(
        fetcher: PublicKeyFetcher,
        public_key_url: Url,
    ) -> Result<Self, VerifierStoreError> {
        fetcher
            .fetch_latest_key(&public_key_url)
            .map(|verifier| Self {
                verifier: Mutex::new(verifier),
                fetcher,
                public_key_url,
            })
            .map_err(|err| VerifierStoreError::Fetch(err.to_string()))
    }

    /// Verifies the signature using the underlying verifier. Such verifier is fetched again if the provided
    /// key_id doesn't match the Verifier's key id.
    pub fn verify_signature(
        &self,
        key_id: &str,
        msg: &[u8],
        signature: &[u8],
    ) -> Result<(), VerifierStoreError> {
        let decoded_signature = BASE64_STANDARD
            .decode(signature)
            .map_err(|e| VerifierStoreError::DecodingSignature(e.to_string()))?;

        let mut verifier = self
            .verifier
            .lock()
            .map_err(|err| VerifierStoreError::VerifySignature(err.to_string()))?;

        if !verifier.key_id().eq_ignore_ascii_case(key_id) {
            debug!("keyId doesn't match, fetching new verifier",);
            *verifier = self
                .fetcher
                .fetch_latest_key(&self.public_key_url)
                .map_err(|err| VerifierStoreError::Fetch(err.to_string()))?;

            if !verifier.key_id().eq_ignore_ascii_case(key_id) {
                return Err(VerifierStoreError::KeyMismatch {
                    signature_key_id: key_id.to_string(),
                    stored_key_id: verifier.key_id().to_string(),
                });
            }
        }

        // Actual implementation from FC side signs the Base64 representation of the SHA256 digest
        // of the message (i.e. the remote configs). Hence, to verify the signature, we need to
        // compute the SHA256 digest of the message, then Base64 encode it, and finally verify
        // the signature against that.
        let msg = digest::digest(&digest::SHA256, msg);
        let msg = BASE64_STANDARD.encode(msg);

        verifier
            .verify_signature(msg.as_bytes(), &decoded_signature)
            .map_err(|e| VerifierStoreError::VerifySignature(e.to_string()))?;

        debug!(
            key_id = verifier.key_id(),
            "signature verification succeeded"
        );

        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use crate::{
        http::{client::HttpClient, config::HttpConfig},
        signature::{public_key::tests::TestKeyPair, public_key_fetcher::tests::FakePubKeyServer},
    };

    use super::*;
    use assert_matches::assert_matches;

    const MESSAGE: &[u8] = b"hello, world";

    /// Sign the message like Fleet does
    ///
    /// Sign the base64 representation of the SHA256 digest of the message (i.e. the remote config).
    fn sign_like_fleet(key_pair: &TestKeyPair, msg: &[u8]) -> Vec<u8> {
        let msg = BASE64_STANDARD.encode(digest::digest(&digest::SHA256, msg));

        // Fleet does encode the signature as Base64, too
        BASE64_STANDARD
            .encode(key_pair.sign(msg.as_bytes()))
            .as_bytes()
            .to_vec()
    }

    #[test]
    fn test_verify_sucess_cache_hit() {
        let key_pair = TestKeyPair::new(0);
        let key_id = key_pair.key_id();
        let signature = sign_like_fleet(&key_pair, MESSAGE);

        let server = FakePubKeyServer::new(vec![key_pair]);
        let fetcher = PublicKeyFetcher::new(HttpClient::new(HttpConfig::default()).unwrap());

        let store = VerifierStore::try_new(fetcher, server.url.clone()).unwrap();
        store
            .verify_signature(key_id.as_str(), MESSAGE, &signature)
            .expect("Signature verification should success");
    }

    #[test]
    fn test_verify_sucess_cache_miss() {
        use httpmock::prelude::*;
        use serde_json::json;

        let key_pair_0 = TestKeyPair::new(0);
        let key_pair_1 = TestKeyPair::new(1);

        let mock_server = MockServer::start();

        let mut first_mock = mock_server.mock(|when, then| {
            when.method(GET).path("/jwks");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "keys": [key_pair_0.public_key_jwk()]
                }));
        });

        let http_client = HttpClient::new(HttpConfig::default()).unwrap();
        let fetcher = PublicKeyFetcher::new(http_client);
        let jwks_url = Url::parse(&format!("{}/jwks", mock_server.base_url())).unwrap();

        let store = VerifierStore::try_new(fetcher, jwks_url).unwrap();

        // Remove first mock and add second one that returns key_pair_2
        first_mock.delete();
        mock_server.mock(|when, then| {
            when.method(GET).path("/jwks");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "keys": [key_pair_1.public_key_jwk(), key_pair_0.public_key_jwk()]
                }));
        });

        // Verify with key_pair_2's key_id - triggers refetch, gets key_pair_2
        store
            .verify_signature(
                &key_pair_1.key_id(),
                MESSAGE,
                &sign_like_fleet(&key_pair_1, MESSAGE),
            )
            .expect("Should succeed after refetch");
    }

    #[test]
    fn test_signature_decode_fail() {
        let key_pair_0 = TestKeyPair::new(0);
        let key_id = key_pair_0.key_id();

        let server = FakePubKeyServer::new(vec![key_pair_0]);
        let fetcher = PublicKeyFetcher::new(HttpClient::new(HttpConfig::default()).unwrap());

        let store = VerifierStore::try_new(fetcher, server.url).unwrap();
        let result = store.verify_signature(key_id.as_str(), MESSAGE, b"not-base-64");
        assert_matches!(result, Err(VerifierStoreError::DecodingSignature(_)));
    }

    #[test]
    fn test_signature_check_mismatch() {
        let key_pair_0 = TestKeyPair::new(0);
        let key_id = key_pair_0.key_id();

        let server = FakePubKeyServer::new(vec![key_pair_0]);
        let fetcher = PublicKeyFetcher::new(HttpClient::new(HttpConfig::default()).unwrap());

        let store = VerifierStore::try_new(fetcher, server.url.clone()).unwrap();
        let result = store.verify_signature(
            key_id.as_str(),
            MESSAGE,
            encode_signature(b"signature").as_bytes(),
        );
        assert_matches!(result, Err(VerifierStoreError::VerifySignature(_)));
    }

    /// Generates a payload to be signed as FC does for remote configs blobs.
    pub fn config_signature_payload(msg: &[u8]) -> Vec<u8> {
        let digest = digest::digest(&digest::SHA256, msg);
        BASE64_STANDARD.encode(digest).as_bytes().to_vec()
    }

    fn encode_signature(s: &[u8]) -> String {
        BASE64_STANDARD.encode(s)
    }
}
