use super::certificate::Certificate;
use super::certificate_fetcher::CertificateFetcher;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use std::sync::Mutex;
use thiserror::Error;
use tracing::log::error;

#[derive(Error, Debug, PartialEq)]
pub enum CertificateStoreError {
    #[error("fetching certificate: `{0}`")]
    CertificateFetch(String),
    #[error("signature keyId({signature_key_id}) does not match certificate keyId({certificate_key_id})")]
    KeyMismatch {
        signature_key_id: String,
        certificate_key_id: String,
    },
    #[error("validating signature: `{0}`")]
    VerifySignature(String),
    #[error("decoding signature: `{0}`")]
    DecodingSignature(String),
}

/// The CertificateStore is responsible for fetching and holding the certificate
/// used to verify remote configurations.
pub struct CertificateStore {
    certificate: Mutex<Certificate>,
    fetcher: CertificateFetcher,
}

impl CertificateStore {
    pub fn try_new(fetcher: CertificateFetcher) -> Result<Self, CertificateStoreError> {
        fetcher
            .fetch()
            .map(|certificate| Self {
                certificate: Mutex::new(certificate),
                fetcher,
            })
            .map_err(|e| CertificateStoreError::CertificateFetch(e.to_string()))
    }

    /// Verify the signature of the given message using the stored certificate.
    /// The signature is expected to be in standard base64 encoding.
    /// Fails if the signature is not valid or the key_id does not match the certificate's public key id.
    pub fn verify_signature(
        &self,
        algorithm: &webpki::SignatureAlgorithm,
        key_id: &str,
        msg: &[u8],
        signature: &[u8],
    ) -> Result<(), CertificateStoreError> {
        let decoded_signature = BASE64_STANDARD
            .decode(signature)
            .map_err(|e| CertificateStoreError::DecodingSignature(e.to_string()))?;

        let cert = self.get_certificate(key_id)?;

        cert.verify_signature(algorithm, msg, &decoded_signature)
            .map_err(|e| CertificateStoreError::VerifySignature(e.to_string()))
    }

    /// Gets the stored certificate if the key_id matches the certificate's public key id.
    /// If the key_id does not match, fetch the certificate again assuming the certificate has been rotated.
    /// Fails if the key_id does not match the certificate's public key id after fetching the certificate.
    fn get_certificate(
        &self,
        signature_key_id: &str,
    ) -> Result<Certificate, CertificateStoreError> {
        let mut certificate = self
            .certificate
            .lock()
            .map_err(|e| CertificateStoreError::VerifySignature(e.to_string()))?;

        if certificate
            .public_key_id()
            .eq_ignore_ascii_case(signature_key_id)
        {
            return Ok(certificate.clone());
        }

        // If the key_id does not match, fetch the certificate again assuming the certificate has been rotated.
        *certificate = self
            .fetcher
            .fetch()
            .map_err(|e| CertificateStoreError::CertificateFetch(e.to_string()))?;

        if !certificate
            .public_key_id()
            .eq_ignore_ascii_case(signature_key_id)
        {
            return Err(CertificateStoreError::KeyMismatch {
                signature_key_id: signature_key_id.to_string(),
                certificate_key_id: certificate.public_key_id().to_string(),
            });
        }

        Ok(certificate.clone())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::http::tls::install_rustls_default_crypto_provider;
    use assert_matches::assert_matches;
    use rcgen::{CertificateParams, PKCS_ED25519};
    use std::path::PathBuf;
    use tempfile::TempDir;
    use webpki::{ED25519, RSA_PKCS1_2048_8192_SHA512};

    // A test signer util that generates a key pair and a self-signed certificate, and can be used to sign messages,
    // as the OpAmp server would do.
    // The certificate is written to a temporary file which is cleaned up when the signer is dropped.
    pub struct TestSigner {
        key_pair: rcgen::KeyPair,
        cert_temp_dir: TempDir,
        cert: rcgen::Certificate,
        key_id: String,
    }
    impl TestSigner {
        const CERT_FILE_NAME: &'static str = "test.pem";
        pub fn new() -> Self {
            let key_pair = rcgen::KeyPair::generate_for(&PKCS_ED25519).unwrap();
            let cert = CertificateParams::new(vec!["localhost".to_string()])
                .unwrap()
                .self_signed(&key_pair)
                .unwrap();

            let key_id = Certificate::try_new(cert.der().as_ref().to_vec())
                .unwrap()
                .public_key_id()
                .to_string();

            let cert_temp_dir = tempfile::tempdir().unwrap();
            std::fs::write(cert_temp_dir.path().join(Self::CERT_FILE_NAME), cert.pem()).unwrap();

            Self {
                key_pair,
                key_id,
                cert,
                cert_temp_dir,
            }
        }
        pub fn cert_pem_path(&self) -> PathBuf {
            self.cert_temp_dir.path().join(Self::CERT_FILE_NAME)
        }
        pub fn key_id(&self) -> &str {
            &self.key_id
        }
        pub fn cert_pem(&self) -> String {
            self.cert.pem()
        }
        /// Sign a message and encode the signature in standard base64 encoding.
        pub fn encoded_signature(&self, msg: &str) -> String {
            let key_pair_ring =
                ring::signature::Ed25519KeyPair::from_pkcs8(&self.key_pair.serialize_der())
                    .unwrap();
            let signature = key_pair_ring.sign(msg.as_bytes());
            BASE64_STANDARD.encode(signature.as_ref())
        }
    }

    #[test]
    fn test_verify_sucess() {
        install_rustls_default_crypto_provider();
        let test_signer = TestSigner::new();
        let config = "fake_config";

        let cert_store =
            CertificateStore::try_new(CertificateFetcher::PemFile(test_signer.cert_pem_path()))
                .unwrap();

        cert_store
            .verify_signature(
                &ED25519,
                test_signer.key_id(),
                config.as_bytes(),
                test_signer.encoded_signature(config).as_bytes(),
            )
            .unwrap();
    }
    #[test]
    fn test_signature_content_missmatch() {
        install_rustls_default_crypto_provider();
        let test_signer = TestSigner::new();

        let cert_store =
            CertificateStore::try_new(CertificateFetcher::PemFile(test_signer.cert_pem_path()))
                .unwrap();

        let err = cert_store
            .verify_signature(
                &ED25519,
                test_signer.key_id(),
                b"some config",
                test_signer
                    .encoded_signature("some other config")
                    .as_bytes(),
            )
            .unwrap_err();

        assert_matches!(err, CertificateStoreError::VerifySignature(_));
    }
    #[test]
    fn test_signature_algorithm_missmatch() {
        install_rustls_default_crypto_provider();
        let test_signer = TestSigner::new();
        let config = "fake_config";

        let cert_store =
            CertificateStore::try_new(CertificateFetcher::PemFile(test_signer.cert_pem_path()))
                .unwrap();

        let err = cert_store
            .verify_signature(
                &RSA_PKCS1_2048_8192_SHA512,
                test_signer.key_id(),
                config.as_bytes(),
                test_signer.encoded_signature(config).as_bytes(),
            )
            .unwrap_err();

        assert_matches!(err, CertificateStoreError::VerifySignature(_));
    }
    #[test]
    fn test_signature_encode_fail() {
        install_rustls_default_crypto_provider();
        let test_signer = TestSigner::new();

        let cert_store =
            CertificateStore::try_new(CertificateFetcher::PemFile(test_signer.cert_pem_path()))
                .unwrap();

        let err = cert_store
            .verify_signature(
                &ED25519,
                test_signer.key_id(),
                b"some config",
                b"not base 64",
            )
            .unwrap_err();

        assert_matches!(err, CertificateStoreError::DecodingSignature(_));
    }
    #[test]
    fn test_signature_key_missmatch() {
        install_rustls_default_crypto_provider();
        let test_signer = TestSigner::new();

        let cert_store =
            CertificateStore::try_new(CertificateFetcher::PemFile(test_signer.cert_pem_path()))
                .unwrap();

        let err = cert_store
            .verify_signature(
                &ED25519,
                "123",
                b"fake",
                test_signer
                    .encoded_signature("some other config")
                    .as_bytes(),
            )
            .unwrap_err();

        assert_eq!(
            err,
            CertificateStoreError::KeyMismatch {
                signature_key_id: "123".to_string(),
                certificate_key_id: test_signer.key_id().to_string(),
            }
        );
    }
    #[test]
    fn cache_hit() {
        install_rustls_default_crypto_provider();
        let test_signer = TestSigner::new();
        let config = "fake_config";
        let config_signature = test_signer.encoded_signature(config);

        let fetcher = CertificateFetcher::PemFile(test_signer.cert_pem_path());
        let cert_store = CertificateStore::try_new(fetcher).unwrap();

        let key_id = test_signer.key_id().to_string();
        cert_store
            .verify_signature(
                &ED25519,
                &key_id,
                config.as_bytes(),
                config_signature.as_bytes(),
            )
            .unwrap();
        // dropping the test signer cleans the certificate pem file form disk
        let copy_of_cert_path = test_signer.cert_pem_path();
        drop(test_signer);
        assert!(!copy_of_cert_path.exists());
        // Verifies the fetcher fails to fetch the certificate from file
        CertificateFetcher::PemFile(copy_of_cert_path)
            .fetch()
            .unwrap_err();

        // verify with the cached certificate
        cert_store
            .verify_signature(
                &ED25519,
                &key_id,
                config.as_bytes(),
                config_signature.as_bytes(),
            )
            .unwrap();
    }
    #[test]
    fn cache_miss() {
        install_rustls_default_crypto_provider();
        let test_signer_first = TestSigner::new();
        let config = "fake_config";

        let cert_temp_dir = tempfile::tempdir().unwrap();
        let certificate_path = cert_temp_dir.path().join("test.crt");
        std::fs::write(&certificate_path, test_signer_first.cert_pem()).unwrap();

        let fetcher = CertificateFetcher::PemFile(certificate_path.to_path_buf());
        let cert_store = CertificateStore::try_new(fetcher).unwrap();
        // Make sure the cache is populated
        cert_store
            .verify_signature(
                &ED25519,
                test_signer_first.key_id(),
                config.as_bytes(),
                test_signer_first.encoded_signature(config).as_bytes(),
            )
            .unwrap();

        // Create a new certificate and update the file on disk where the fetcher is reading from.
        let test_signer_second = TestSigner::new();
        std::fs::write(&certificate_path, test_signer_second.cert_pem()).unwrap();

        // Verifies a signature with the new certificate
        cert_store
            .verify_signature(
                &ED25519,
                test_signer_second.key_id(),
                config.as_bytes(),
                test_signer_second.encoded_signature(config).as_bytes(),
            )
            .unwrap();
    }
}
