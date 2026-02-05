use base64::{Engine, prelude::BASE64_STANDARD};
use ring::digest;
use std::sync::Mutex;
use thiserror::Error;
use tracing::debug;

use crate::signature::public_key::{PublicKey, SigningAlgorithm};
use crate::signature::public_key_fetcher::PublicKeyFetcher;

/// Represents any struct that is able to verify signatures and it is identified by a key.
pub trait Verifier {
    type Error: std::error::Error;

    fn verify_signature(
        &self,
        algorithm: &SigningAlgorithm,
        msg: &[u8],
        signature: &[u8],
    ) -> Result<(), Self::Error>;

    fn key_id(&self) -> &str;
}

impl Verifier for PublicKey {
    type Error = VerifierStoreError;

    fn verify_signature(
        &self,
        signing_algorithm: &SigningAlgorithm,
        msg: &[u8],
        signature: &[u8],
    ) -> Result<(), Self::Error> {
        // Actual implementation from FC side signs the Base64 representation of the SHA256 digest
        // of the message (i.e. the remote configs). Hence, to verify the signature, we need to
        // compute the SHA256 digest of the message, then Base64 encode it, and finally verify
        // the signature against that.
        let msg = digest::digest(&digest::SHA256, msg);
        let msg = BASE64_STANDARD.encode(msg);

        self.verify_signature(signing_algorithm, msg.as_bytes(), signature)
            .map_err(|e| VerifierStoreError::VerifySignature(e.to_string()))?;

        debug!(key_id = self.key_id(), "signature verification succeeded");

        Ok(())
    }

    fn key_id(&self) -> &str {
        self.key_id()
    }
}

/// Defines how to fetch a new [Verifier].
pub trait VerifierFetcher {
    type Error: std::error::Error;
    type Verifier: Verifier;

    fn fetch(&self) -> Result<Self::Verifier, Self::Error>;
}

impl VerifierFetcher for PublicKeyFetcher {
    type Error = VerifierStoreError;
    type Verifier = PublicKey;
    fn fetch(&self) -> Result<Self::Verifier, Self::Error> {
        self.fetch_latest_key()
            .map_err(|e| VerifierStoreError::Fetch(e.to_string()))
    }
}

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
pub struct VerifierStore<V, F>
where
    V: Verifier,
    F: VerifierFetcher<Verifier = V>,
{
    verifier: Mutex<V>,
    fetcher: F,
}

impl<V, F> VerifierStore<V, F>
where
    V: Verifier,
    F: VerifierFetcher<Verifier = V>,
{
    pub fn try_new(fetcher: F) -> Result<Self, VerifierStoreError> {
        fetcher
            .fetch()
            .map(|verifier| Self {
                verifier: Mutex::new(verifier),
                fetcher,
            })
            .map_err(|err| VerifierStoreError::Fetch(err.to_string()))
    }

    /// Verifies the signature using the underlying verifier. Such verifier is fetched again if the provided
    /// key_id doesn't match the Verifier's key id.
    pub fn verify_signature(
        &self,
        algorithm: &SigningAlgorithm,
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
                .fetch()
                .map_err(|err| VerifierStoreError::Fetch(err.to_string()))?;

            if !verifier.key_id().eq_ignore_ascii_case(key_id) {
                return Err(VerifierStoreError::KeyMismatch {
                    signature_key_id: key_id.to_string(),
                    stored_key_id: verifier.key_id().to_string(),
                });
            }
        }

        verifier
            .verify_signature(algorithm, msg, &decoded_signature)
            .map_err(|err| VerifierStoreError::VerifySignature(err.to_string()))
    }
}

#[cfg(test)]
pub mod tests {
    use crate::signature::public_key::tests::TestKeyPair;

    use super::*;
    use actix_web::Result;
    use assert_matches::assert_matches;
    use mockall::{Sequence, mock};

    #[derive(Debug, Error)]
    #[error("some error: {0}")]
    pub struct MockVerifierError(String);

    mock! {
        pub Verifier {}

        impl Verifier for Verifier {
            type Error = MockVerifierError;
            fn verify_signature(
                &self,
                algorithm: &SigningAlgorithm,
                msg: &[u8],
                signature: &[u8],
            ) -> Result<(), <Self as Verifier>::Error>;

            fn key_id(&self) -> &str;
        }
    }

    mock! {
        pub VerifierFetcher {}

        impl VerifierFetcher for VerifierFetcher {
            type Error = MockVerifierError;
            type Verifier = MockVerifier;

            fn fetch(&self) -> Result<<Self as VerifierFetcher>::Verifier, <Self as VerifierFetcher>::Error>;
        }
    }

    // VerifierStore tests

    #[test]
    fn test_verify_sucess_cache_hit() {
        const KEY_ID: &str = "key-id";
        let mut fetcher = MockVerifierFetcher::new();
        fetcher.expect_fetch().once().returning(|| {
            let mut verifier = MockVerifier::new();
            verifier
                .expect_key_id()
                .once()
                .return_const(KEY_ID.to_string());
            verifier
                .expect_verify_signature()
                .once()
                .returning(|_, _, _| Ok(()));
            Ok(verifier)
        });

        let store = VerifierStore::try_new(fetcher).unwrap();
        store
            .verify_signature(
                &SigningAlgorithm::ED25519,
                KEY_ID,
                b"some-message",
                encode_signature(b"signature").as_bytes(),
            )
            .expect("Signature verification should success");
    }

    #[test]
    fn test_verify_sucess_cache_miss() {
        const KEY_ID1: &str = "key-id-1";
        const KEY_ID2: &str = "key-id-2";
        let mut fetcher = MockVerifierFetcher::new();
        let mut seq = Sequence::new();
        fetcher
            .expect_fetch()
            .once()
            .in_sequence(&mut seq)
            .returning(|| {
                let mut verifier = MockVerifier::new();
                verifier
                    .expect_key_id()
                    .once()
                    .return_const(KEY_ID1.to_string());

                Ok(verifier)
            });
        fetcher
            .expect_fetch()
            .once()
            .in_sequence(&mut seq)
            .returning(|| {
                let mut verifier = MockVerifier::new();
                verifier
                    .expect_key_id()
                    .once()
                    .return_const(KEY_ID2.to_string());
                verifier
                    .expect_verify_signature()
                    .once()
                    .returning(|_, _, _| Ok(()));
                Ok(verifier)
            });

        let store = VerifierStore::try_new(fetcher).unwrap();
        store
            .verify_signature(
                &SigningAlgorithm::ED25519,
                KEY_ID2,
                b"some-message",
                encode_signature(b"signature").as_bytes(),
            )
            .expect("Signature verification should success");
    }

    #[test]
    fn test_signature_decode_fail() {
        const KEY_ID: &str = "key-id";
        let mut fetcher = MockVerifierFetcher::new();
        fetcher
            .expect_fetch()
            .once()
            .returning(|| Ok(MockVerifier::new()));

        let store = VerifierStore::try_new(fetcher).unwrap();
        let result = store.verify_signature(
            &SigningAlgorithm::ED25519,
            KEY_ID,
            b"some-message",
            b"not-base-64",
        );
        assert_matches!(result, Err(VerifierStoreError::DecodingSignature(_)));
    }

    #[test]
    fn test_signature_check_mismatch() {
        const KEY_ID: &str = "key-id";
        let mut fetcher = MockVerifierFetcher::new();
        fetcher.expect_fetch().once().returning(|| {
            let mut verifier = MockVerifier::new();
            verifier
                .expect_key_id()
                .once()
                .return_const(KEY_ID.to_string());
            verifier
                .expect_verify_signature()
                .once()
                .returning(|_, _, _| Err(MockVerifierError("invalid signature".to_string())));
            Ok(verifier)
        });

        let store = VerifierStore::try_new(fetcher).unwrap();
        let result = store.verify_signature(
            &SigningAlgorithm::ED25519,
            KEY_ID,
            b"some-message",
            encode_signature(b"signature").as_bytes(),
        );
        assert_matches!(result, Err(VerifierStoreError::VerifySignature(_)));
    }

    // Verifier tests
    #[test]
    fn test_verify() {
        let key_pair = TestKeyPair::new(0);
        let pub_key = key_pair.public_key();
        const MESSAGE: &[u8] = b"hello, world";

        let signature = key_pair.sign(&config_signature_payload(MESSAGE));

        <PublicKey as Verifier>::verify_signature(
            &pub_key,
            &SigningAlgorithm::ED25519,
            MESSAGE,
            &signature,
        )
        .unwrap();
    }

    #[test]
    fn test_verify_wrong_signature() {
        let key_pair = TestKeyPair::new(0);
        let pub_key = key_pair.public_key();
        const MESSAGE: &[u8] = b"hello, world";

        let signature = key_pair.sign(b"some other message");
        assert_matches!(
            <PublicKey as Verifier>::verify_signature(
                &pub_key,
                &SigningAlgorithm::ED25519,
                MESSAGE,
                &signature,
            )
            .unwrap_err(),
            VerifierStoreError::VerifySignature(_)
        );
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
