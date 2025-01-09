use super::certificate_fetcher::{CertificateFetcher, DerCertificateBytes};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use std::sync::Mutex;
use thiserror::Error;
use tracing::log::error;
use webpki::EndEntityCert;

#[derive(Error, Debug, PartialEq)]
pub enum CertificateStoreError {
    #[error("fetching certificate: `{0}`")]
    CertificateFetch(String),
    #[error("validating signature: `{0}`")]
    VerifySignature(String),
    #[error("decoding signature: `{0}`")]
    DecodingSignature(String),
}

/// The CertificateStore is responsible for fetching and holding the certificate
/// used to verify remote configurations.
pub struct CertificateStore {
    certificate: Mutex<DerCertificateBytes>,
    #[allow(dead_code)] // TODO will be used when the cache is added
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
    pub fn verify_signature(
        &self,
        algorithm: &webpki::SignatureAlgorithm,
        msg: &[u8],
        signature: &[u8],
    ) -> Result<(), CertificateStoreError> {
        let sig = BASE64_STANDARD
            .decode(signature)
            .map_err(|e| CertificateStoreError::DecodingSignature(e.to_string()))?;

        let der_certificate_bytes = self.get_certificate()?;
        let certificate = EndEntityCert::try_from(der_certificate_bytes.as_slice()).unwrap();

        certificate
            .verify_signature(algorithm, msg, &sig)
            .map_err(|e| CertificateStoreError::VerifySignature(e.to_string()))
    }

    fn get_certificate(&self) -> Result<DerCertificateBytes, CertificateStoreError> {
        // TODO a cache will be added here based on the keyID
        let certificate = self
            .certificate
            .lock()
            .expect("to acquire certificate lock");

        Ok(certificate.clone())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::http::tls::install_rustls_default_crypto_provider;
    use rcgen::{Certificate, CertificateParams, KeyPair, PKCS_ED25519};
    use webpki::{ED25519, RSA_PKCS1_2048_8192_SHA512};

    pub struct TestSigner {
        key_pair: KeyPair,
        cert: Certificate,
    }
    impl TestSigner {
        pub fn new() -> Self {
            let key_pair = KeyPair::generate_for(&PKCS_ED25519).unwrap();
            let cert = CertificateParams::new(vec!["localhost".to_string()])
                .unwrap()
                .self_signed(&key_pair)
                .unwrap();

            Self { key_pair, cert }
        }
        pub fn cert_pem(&self) -> String {
            self.cert.pem()
        }
        pub fn encoded_signature(&self, msg: &str) -> String {
            let key_pair_ring =
                ring::signature::Ed25519KeyPair::from_pkcs8(&self.key_pair.serialize_der())
                    .unwrap();
            let signature = key_pair_ring.sign(msg.as_bytes());
            BASE64_STANDARD.encode(signature.as_ref())
        }
    }

    #[test]
    fn test_verify() {
        install_rustls_default_crypto_provider();

        let test_signer = TestSigner::new();
        let config = r#"fake_config: 1.10.12"#;
        let config_signature = test_signer.encoded_signature(config);

        struct TestCase {
            name: &'static str,
            algorithm: &'static webpki::SignatureAlgorithm,
            config: &'static str,
            config_signature: String,
            expected_result: Result<(), CertificateStoreError>,
        }
        impl TestCase {
            fn run(self, test_signer: &TestSigner) {
                let fetcher = CertificateFetcher::from_pem_string(&test_signer.cert_pem());

                let cert_store = CertificateStore::try_new(fetcher)
                    .unwrap_or_else(|_| panic!("to create store, case: {}", self.name));

                let _ = cert_store
                    .verify_signature(
                        self.algorithm,
                        self.config.as_bytes(),
                        self.config_signature.as_bytes(),
                    )
                    .map(|_| {
                        assert!(
                            self.expected_result.is_ok(),
                            "expected Ok, case: {}",
                            self.name
                        )
                    })
                    .map_err(|err| {
                        assert_eq!(
                            err,
                            self.expected_result.expect_err(
                                format!("error is expected, case: {}", self.name).as_str()
                            )
                        );
                    });
            }
        }
        let test_cases = vec![
            TestCase {
                name: "verify OK",
                algorithm: &ED25519,
                config_signature: config_signature.clone(),
                config,
                expected_result: Ok(()),
            },
            TestCase {
                name: "signature content mismatch",
                algorithm: &ED25519,
                config_signature: config_signature.clone(),
                config: "this is not the config used to sign",
                expected_result: Err(CertificateStoreError::VerifySignature(
                    "InvalidSignatureForPublicKey".to_string(),
                )),
            },
            TestCase {
                name: "signature algorithm mismatch",
                algorithm: &RSA_PKCS1_2048_8192_SHA512,
                config_signature: config_signature.clone(),
                config,
                expected_result: Err(CertificateStoreError::VerifySignature(
                    "UnsupportedSignatureAlgorithmForPublicKey".to_string(),
                )),
            },
            TestCase {
                name: "signature wrong encode",
                algorithm: &ED25519,
                config_signature: "not standard base64".to_string(),
                config,
                expected_result: Err(CertificateStoreError::DecodingSignature(
                    "Invalid symbol 32, offset 3.".to_string(),
                )),
            },
        ];

        for test_case in test_cases {
            test_case.run(&test_signer);
        }
    }
}
