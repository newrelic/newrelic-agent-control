use base64::{Engine, prelude::BASE64_STANDARD};
use std::sync::Mutex;
use thiserror::Error;
use tracing::debug;

/// Represents any struct that is able to verify signatures and it is identified by a key.
pub trait Verifier {
    type Error: std::error::Error;

    fn verify_signature(
        &self,
        algorithm: &webpki::SignatureAlgorithm, // TODO: check if this is this type is compatible with both implementations or we need something else for public-keys
        msg: &[u8],
        signature: &[u8],
    ) -> Result<(), Self::Error>;

    fn key_id(&self) -> &str;
}

/// Defines how to fetch a new [Verifier].
pub trait VerifierFetcher {
    type Error: std::error::Error;
    type Verifier: Verifier;

    fn fetch(&self) -> Result<Self::Verifier, Self::Error>;
}

#[derive(Error, Debug, PartialEq)]
pub enum VerifierStoreError {
    #[error("fetching verifying key: {0}")]
    Fetch(String),
    #[error(
        "signature keyId({signature_key_id}) does not match certificate keyId({certificate_key_id})"
    )]
    KeyMismatch {
        signature_key_id: String,
        certificate_key_id: String,
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
        algorithm: &webpki::SignatureAlgorithm,
        key_id: &str,
        msg: &[u8],
        signature: &[u8],
    ) -> Result<(), VerifierStoreError> {
        let decoded_signature = BASE64_STANDARD
            .decode(signature)
            .map_err(|e| VerifierStoreError::DecodingSignature(e.to_string()))?;

        self.with_verifier(key_id, |verifier| {
            verifier
                .verify_signature(algorithm, msg, &decoded_signature)
                .map_err(|err| VerifierStoreError::VerifySignature(err.to_string()))
        })
    }

    /// Obtains or fetches (depending on the provided `signature_key_id`) the verifier executes the provided callback.
    fn with_verifier<T: Fn(&V) -> Result<(), VerifierStoreError>>(
        &self,
        signature_key_id: &str,
        f: T,
    ) -> Result<(), VerifierStoreError> {
        let mut verifier = self
            .verifier
            .lock()
            .map_err(|err| VerifierStoreError::VerifySignature(err.to_string()))?;

        if verifier.key_id().eq_ignore_ascii_case(signature_key_id) {
            return f(&verifier);
        }

        debug!("Signature's keyId doesn't match the current verifier keyId, fetching new verifier");
        *verifier = self
            .fetcher
            .fetch()
            .map_err(|err| VerifierStoreError::Fetch(err.to_string()))?;

        if !verifier.key_id().eq(signature_key_id) {
            return Err(VerifierStoreError::KeyMismatch {
                signature_key_id: signature_key_id.to_string(),
                certificate_key_id: verifier.key_id().to_string(),
            });
        }

        f(&verifier)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use mockall::{Sequence, mock};
    use webpki::ED25519;

    #[derive(Debug, Error)]
    #[error("some error: {0}")]
    pub struct MockVerifierError(String);

    mock! {
        pub Verifier {}

        impl Verifier for Verifier {
            type Error = MockVerifierError;
            fn verify_signature(
                &self,
                algorithm: &webpki::SignatureAlgorithm,
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
                &ED25519,
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
                &ED25519,
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
        let result = store.verify_signature(&ED25519, KEY_ID, b"some-message", b"not-base-64");
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
            &ED25519,
            KEY_ID,
            b"some-message",
            encode_signature(b"signature").as_bytes(),
        );
        assert_matches!(result, Err(VerifierStoreError::VerifySignature(_)));
    }

    fn encode_signature(s: &[u8]) -> String {
        BASE64_STANDARD.encode(s)
    }
}
