use super::certificate_fetcher::CertificateFetcher;
use crate::agent_control::defaults::get_custom_capabilities;
use crate::http::client::HttpClient;
use crate::http::config::HttpConfig;
use crate::http::config::ProxyConfig;
use crate::opamp::remote_config::OpampRemoteConfig;
use crate::opamp::remote_config::signature::SIGNATURE_CUSTOM_CAPABILITY;
use crate::opamp::remote_config::validators::RemoteConfigValidator;
use crate::opamp::remote_config::validators::signature::certificate::Certificate;
use crate::opamp::remote_config::validators::signature::verifier::VerifierStore;
use crate::sub_agent::identity::AgentIdentity;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;
use tracing::log::error;
use tracing::{info, warn};
use url::Url;

const DEFAULT_CERTIFICATE_SERVER_URL: &str = "https://newrelic.com/";
const DEFAULT_PUBLIC_KEY_SERVER_URL: &str =
    "https://publickeys.newrelic.com/signing/blob-management/GLOBAL/AgentConfiguration";
const DEFAULT_HTTPS_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_SIGNATURE_VALIDATOR_ENABLED: bool = true;

type ErrorMessage = String;
#[derive(Error, Debug)]
pub enum SignatureValidatorError {
    #[error("failed to fetch certificate: `{0}`")]
    FetchCertificate(ErrorMessage),
    #[error("failed to build validator: `{0}`")]
    BuildingValidator(ErrorMessage),
    #[error("failed to verify signature: `{0}`")]
    VerifySignature(ErrorMessage),
}

/// Returns a SignatureValidator wrapping a CertificateSignatureValidator if fleet_control and signature validation are
/// enabled and a no-op validator otherwise.
///
/// Proxies configuration that intercept TLS traffic are not supported since the fetcher expects to connect directly to the server.
pub fn build_signature_validator(
    config: SignatureValidatorConfig,
    proxy_config: ProxyConfig,
) -> Result<SignatureValidator, SignatureValidatorError> {
    if !config.enabled {
        warn!("Remote config signature validation is disabled");
        return Ok(SignatureValidator::Noop);
    }

    // Certificate from file takes precedence over fetching from the server when it is set
    let certificate_fetcher = if !config.certificate_pem_file_path.as_os_str().is_empty() {
        warn!(
            "Remote config signature validation is enabled, using certificate from file: {}. Certificate rotation is not supported",
            config.certificate_pem_file_path.display()
        );
        CertificateFetcher::PemFile(config.certificate_pem_file_path)
    } else {
        info!(
            "Remote config signature validation is enabled, fetching certificate from: {}",
            config.certificate_server_url
        );

        let http_config = HttpConfig::new(
            DEFAULT_HTTPS_CLIENT_TIMEOUT,
            DEFAULT_HTTPS_CLIENT_TIMEOUT,
            proxy_config,
        )
        .with_tls_info();

        let client = HttpClient::new(http_config)
            .map_err(|e| SignatureValidatorError::BuildingValidator(e.to_string()))?;

        CertificateFetcher::Https(config.certificate_server_url, client)
    };

    let verifier_store = VerifierStore::try_new(certificate_fetcher)
        .map_err(|err| SignatureValidatorError::BuildingValidator(err.to_string()))?;

    Ok(SignatureValidator::Certificate(
        CertificateSignatureValidator::new(verifier_store),
    ))
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct SignatureValidatorConfig {
    #[serde(default = "default_certificate_server_url")]
    pub certificate_server_url: Url,
    #[serde(default = "default_public_key_server_url")]
    pub public_key_server_url: Url,
    /// Path to the PEM file containing the certificate to validate the signature.
    /// Takes precedence over fetching from the server when it is set
    #[serde(default)]
    pub certificate_pem_file_path: PathBuf,
    #[serde(default = "default_signature_validator_config_enabled")]
    pub enabled: bool,
}

impl Default for SignatureValidatorConfig {
    fn default() -> Self {
        Self {
            enabled: default_signature_validator_config_enabled(),
            certificate_server_url: default_certificate_server_url(),
            public_key_server_url: default_public_key_server_url(),
            certificate_pem_file_path: PathBuf::new(),
        }
    }
}

fn default_certificate_server_url() -> Url {
    Url::parse(DEFAULT_CERTIFICATE_SERVER_URL).unwrap_or_else(|err| {
        panic!("Invalid DEFAULT_CERTIFICATE_SERVER_URL: '{DEFAULT_CERTIFICATE_SERVER_URL}': {err}")
    })
}

fn default_public_key_server_url() -> Url {
    Url::parse(DEFAULT_PUBLIC_KEY_SERVER_URL).unwrap_or_else(|err| {
        panic!("Invalid DEFAULT_PUBLIC_KEY_SERVER_URL: '{DEFAULT_PUBLIC_KEY_SERVER_URL}': {err}")
    })
}

fn default_signature_validator_config_enabled() -> bool {
    DEFAULT_SIGNATURE_VALIDATOR_ENABLED
}

// NOTE: if we updated the components using the validator to use a composite-like implementation to handle validation,
// the no-op validator wouldn't be necessary.
/// The SignatureValidator enum wraps [CertificateSignatureValidator] and adds support for No-op validator.
pub enum SignatureValidator {
    Certificate(CertificateSignatureValidator),
    PublicKey, // TODO: build this variant from configuration
    Noop,
}

impl RemoteConfigValidator for SignatureValidator {
    type Err = SignatureValidatorError;

    fn validate(
        &self,
        agent_identity: &AgentIdentity,
        opamp_remote_config: &OpampRemoteConfig,
    ) -> Result<(), Self::Err> {
        match self {
            SignatureValidator::Certificate(v) => v.validate(agent_identity, opamp_remote_config),
            SignatureValidator::PublicKey => {
                // TODO: currently it is equivalent to Noop, a future iteration will implement its validation that could have a
                // fallback to Certificate
                Ok(())
            }
            SignatureValidator::Noop => Ok(()),
        }
    }
}

/// The CertificateSignatureValidator is responsible for checking the validity of the signature.
pub struct CertificateSignatureValidator {
    certificate_store: VerifierStore<Certificate, CertificateFetcher>,
}

impl CertificateSignatureValidator {
    pub fn new(certificate_store: VerifierStore<Certificate, CertificateFetcher>) -> Self {
        Self { certificate_store }
    }
}

impl RemoteConfigValidator for CertificateSignatureValidator {
    type Err = SignatureValidatorError;

    fn validate(
        &self,
        agent_identity: &AgentIdentity,
        opamp_remote_config: &OpampRemoteConfig,
    ) -> Result<(), SignatureValidatorError> {
        // custom capabilities are got from the agent-type (currently hard-coded)
        // If the capability is not set, no validation is performed
        if !get_custom_capabilities(&agent_identity.agent_type_id).is_some_and(|c| {
            c.capabilities
                .contains(&SIGNATURE_CUSTOM_CAPABILITY.to_string())
        }) {
            return Ok(());
        }

        let signature = opamp_remote_config
            .get_unique_signature()
            .map_err(|e| SignatureValidatorError::VerifySignature(e.to_string()))?
            .ok_or(SignatureValidatorError::VerifySignature(
                "Signature is missing".to_string(),
            ))?;

        let config_content = opamp_remote_config
            .get_unique()
            .map_err(|e| SignatureValidatorError::VerifySignature(e.to_string()))?
            .as_bytes();

        self.certificate_store
            .verify_signature(
                signature.signature_algorithm(),
                signature.key_id(),
                config_content,
                signature.signature(),
            )
            .map_err(|e| SignatureValidatorError::VerifySignature(e.to_string()))
    }
}

#[cfg(test)]
pub mod tests {
    use std::collections::HashMap;
    use std::str::FromStr;

    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::http::tls::install_rustls_default_crypto_provider;
    use crate::opamp::remote_config::ConfigurationMap;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::opamp::remote_config::signature::{
        ECDSA_P256_SHA256, ED25519, SignatureData, Signatures, SigningAlgorithm,
    };
    use crate::opamp::remote_config::validators::signature::verifier::{
        Verifier, VerifierStoreError,
    };
    use crate::sub_agent::identity::AgentIdentity;
    use assert_matches::assert_matches;
    use base64::Engine;
    use base64::prelude::BASE64_STANDARD;
    use rcgen::{CertificateParams, PKCS_ED25519};
    use tempfile::TempDir;

    // A test signer util that generates a key pair and a self-signed certificate, and can be used to sign messages,
    // as the OpAmp server would do.
    // The certificate is written to a temporary file which is cleaned up when the signer is dropped.
    pub struct TestCertificateSigner {
        key_pair: rcgen::KeyPair,
        cert_temp_dir: TempDir,
        cert: rcgen::Certificate,
        key_id: String,
    }
    impl TestCertificateSigner {
        const CERT_FILE_NAME: &'static str = "test.pem";
        pub fn new() -> Self {
            let key_pair = rcgen::KeyPair::generate_for(&PKCS_ED25519).unwrap();
            let cert = CertificateParams::new(vec!["localhost".to_string()])
                .unwrap()
                .self_signed(&key_pair)
                .unwrap();

            let key_id = Certificate::try_new(cert.der().as_ref().to_vec())
                .unwrap()
                .key_id()
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

    impl Default for TestCertificateSigner {
        fn default() -> Self {
            Self::new()
        }
    }

    #[test]
    fn test_certificate_verify_sucess() {
        install_rustls_default_crypto_provider();
        let test_signer = TestCertificateSigner::new();
        let config = "fake_config";

        let cert_store =
            VerifierStore::try_new(CertificateFetcher::PemFile(test_signer.cert_pem_path()))
                .unwrap();

        cert_store
            .verify_signature(
                &SigningAlgorithm::ED25519,
                test_signer.key_id(),
                config.as_bytes(),
                test_signer.encoded_signature(config).as_bytes(),
            )
            .unwrap();
    }
    #[test]

    fn test_certificate_signature_content_missmatch() {
        install_rustls_default_crypto_provider();
        let test_signer = TestCertificateSigner::new();

        let cert_store =
            VerifierStore::try_new(CertificateFetcher::PemFile(test_signer.cert_pem_path()))
                .unwrap();

        let err = cert_store
            .verify_signature(
                &SigningAlgorithm::ED25519,
                test_signer.key_id(),
                b"some config",
                test_signer
                    .encoded_signature("some other config")
                    .as_bytes(),
            )
            .unwrap_err();

        assert_matches!(err, VerifierStoreError::VerifySignature(_));
    }

    #[test]
    fn test_certificate_signature_algorithm_missmatch() {
        install_rustls_default_crypto_provider();
        let test_signer = TestCertificateSigner::new();
        let config = "fake_config";

        let cert_store =
            VerifierStore::try_new(CertificateFetcher::PemFile(test_signer.cert_pem_path()))
                .unwrap();

        let err = cert_store
            .verify_signature(
                &SigningAlgorithm::RSA_PKCS1_2048_8192_SHA512,
                test_signer.key_id(),
                config.as_bytes(),
                test_signer.encoded_signature(config).as_bytes(),
            )
            .unwrap_err();

        assert_matches!(err, VerifierStoreError::VerifySignature(_));
    }

    #[test]
    fn test_default_signature_validator_config() {
        let config = SignatureValidatorConfig::default();
        assert_eq!(
            config.certificate_server_url.to_string(),
            DEFAULT_CERTIFICATE_SERVER_URL
        );
        assert_eq!(config.enabled, DEFAULT_SIGNATURE_VALIDATOR_ENABLED)
    }

    #[test]
    fn test_signature_validator_config() {
        struct TestCase {
            name: &'static str,
            cfg: &'static str,
            expected: SignatureValidatorConfig,
        }

        impl TestCase {
            fn run(self) {
                let config: SignatureValidatorConfig = serde_yaml::from_str(self.cfg)
                    .unwrap_or_else(|err| {
                        panic!("{} - Invalid config '{}': {}", self.name, self.cfg, err)
                    });
                assert_eq!(config, self.expected, "{} failed", self.name);
            }
        }

        let test_cases = [
            TestCase {
                name: "Setup enabled only (false)",
                cfg: r#"
enabled: false
"#,
                expected: SignatureValidatorConfig {
                    enabled: false,
                    certificate_server_url: Url::parse(DEFAULT_CERTIFICATE_SERVER_URL).unwrap(),
                    public_key_server_url: Url::parse(DEFAULT_PUBLIC_KEY_SERVER_URL).unwrap(),
                    certificate_pem_file_path: PathBuf::new(),
                },
            },
            TestCase {
                name: "Setup enabled only (true)",
                cfg: r#"
enabled: true
"#,
                expected: SignatureValidatorConfig {
                    enabled: true,
                    certificate_server_url: Url::parse(DEFAULT_CERTIFICATE_SERVER_URL).unwrap(),
                    public_key_server_url: Url::parse(DEFAULT_PUBLIC_KEY_SERVER_URL).unwrap(),
                    certificate_pem_file_path: PathBuf::new(),
                },
            },
            TestCase {
                name: "Setup url only",
                cfg: r#"
certificate_server_url: https://example.com
"#,
                expected: SignatureValidatorConfig {
                    enabled: DEFAULT_SIGNATURE_VALIDATOR_ENABLED,
                    certificate_server_url: Url::parse("https://example.com").unwrap(),
                    public_key_server_url: Url::parse(DEFAULT_PUBLIC_KEY_SERVER_URL).unwrap(),
                    certificate_pem_file_path: PathBuf::new(),
                },
            },
            TestCase {
                name: "Setup url and enabled",
                cfg: r#"
enabled: true
certificate_server_url: https://example.com
"#,
                expected: SignatureValidatorConfig {
                    enabled: true,
                    certificate_server_url: Url::parse("https://example.com").unwrap(),
                    public_key_server_url: Url::parse(DEFAULT_PUBLIC_KEY_SERVER_URL).unwrap(),
                    certificate_pem_file_path: PathBuf::new(),
                },
            },
            TestCase {
                name: "Setup file and enabled",
                cfg: r#"
enabled: true
certificate_pem_file_path: /path/to/file
"#,
                expected: SignatureValidatorConfig {
                    enabled: true,
                    certificate_server_url: Url::parse(DEFAULT_CERTIFICATE_SERVER_URL).unwrap(),
                    public_key_server_url: Url::parse(DEFAULT_PUBLIC_KEY_SERVER_URL).unwrap(),
                    certificate_pem_file_path: PathBuf::from_str("/path/to/file").unwrap(),
                },
            },
            TestCase {
                name: "Setup file and url and enabled",
                cfg: r#"
enabled: true
certificate_server_url: https://example.com
public_key_server_url: https://test.io
certificate_pem_file_path: /path/to/file
"#,
                expected: SignatureValidatorConfig {
                    enabled: true,
                    certificate_server_url: Url::parse("https://example.com").unwrap(),
                    public_key_server_url: Url::parse("https://test.io").unwrap(),
                    certificate_pem_file_path: PathBuf::from_str("/path/to/file").unwrap(),
                },
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_noop_signature_validator() {
        let rc = OpampRemoteConfig::new(
            AgentID::try_from("test").unwrap(),
            Hash::from("test_payload"),
            ConfigState::Applying,
            None,
        );

        let noop_validator = SignatureValidator::Noop;

        assert!(
            noop_validator
                .validate(&AgentIdentity::default(), &rc)
                .is_ok(),
            "The config should be valid even if the signature is missing when no-op validator is used",
        )
    }

    #[test]
    pub fn test_certificate_signature_validator_err() {
        struct TestCase {
            name: &'static str,
            remote_config: OpampRemoteConfig,
        }

        impl TestCase {
            fn run(self) {
                let test_signer = TestCertificateSigner::new();

                let signature_validator = CertificateSignatureValidator::new(
                    VerifierStore::try_new(CertificateFetcher::PemFile(
                        test_signer.cert_pem_path(),
                    ))
                    .unwrap(),
                );

                let result =
                    signature_validator.validate(&AgentIdentity::default(), &self.remote_config);
                assert_matches!(
                    result,
                    Err(SignatureValidatorError::VerifySignature(_)),
                    "{}",
                    self.name
                );
            }
        }

        let test_cases = [
            TestCase {
                name: "Signature is missing",
                remote_config: OpampRemoteConfig::new(
                    AgentID::try_from("test").unwrap(),
                    Hash::from("test_payload"),
                    ConfigState::Applying,
                    None,
                ),
            },
            TestCase {
                name: "Signature cannot be retrieved because multiple signatures are defined",
                remote_config: OpampRemoteConfig::new(
                    AgentID::try_from("test").unwrap(),
                    Hash::from("test_payload"),
                    ConfigState::Applying,
                    None,
                )
                .with_signature(Signatures::new_multiple([
                    SignatureData::new("first", ED25519, "fake_key_id"),
                    SignatureData::new("second", ED25519, "fake_key_id"),
                ])),
            },
            TestCase {
                name: "Config is empty",
                remote_config: OpampRemoteConfig::new(
                    AgentID::try_from("test").unwrap(),
                    Hash::from("test_payload"),
                    ConfigState::Applying,
                    None,
                )
                .with_signature(Signatures::new_unique("", ED25519, "fake_key_id")),
            },
            TestCase {
                name: "Invalid signature",
                remote_config: OpampRemoteConfig::new(
                    AgentID::try_from("test").unwrap(),
                    Hash::from("test_payload"),
                    ConfigState::Applying,
                    Some(ConfigurationMap::new(HashMap::from([(
                        "key".to_string(),
                        "value".to_string(),
                    )]))),
                )
                .with_signature(Signatures::new_unique(
                    "invalid signature",
                    ECDSA_P256_SHA256,
                    "fake_key_id",
                )),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    pub fn test_certificate_signature_validator_signature_is_missing_for_agent_control_agent() {
        let test_signer = TestCertificateSigner::new();
        let signature_validator = CertificateSignatureValidator::new(
            VerifierStore::try_new(CertificateFetcher::PemFile(test_signer.cert_pem_path()))
                .unwrap(),
        );
        let rc = OpampRemoteConfig::new(
            AgentID::AgentControl,
            Hash::from("test"),
            ConfigState::Applying,
            None,
        );
        // Signature custom capability is not set for agent-control agent, therefore signature is not checked
        assert!(
            signature_validator
                .validate(&AgentIdentity::new_agent_control_identity(), &rc)
                .is_ok()
        );
    }

    #[test]
    pub fn test_certificate_signature_validator_signature_is_valid() {
        let test_signer = TestCertificateSigner::new();

        let signature_validator = CertificateSignatureValidator::new(
            VerifierStore::try_new(CertificateFetcher::PemFile(test_signer.cert_pem_path()))
                .unwrap(),
        );

        let config = "value";

        let encoded_signature = test_signer.encoded_signature(config);
        let remote_config = OpampRemoteConfig::new(
            AgentID::AgentControl,
            Hash::from("test"),
            ConfigState::Applying,
            Some(ConfigurationMap::new(HashMap::from([(
                "key".into(),
                config.to_string(),
            )]))),
        )
        .with_signature(Signatures::new_unique(
            encoded_signature.as_str(),
            ED25519, // Test signer uses this algorithm
            test_signer.key_id(),
        ));

        assert!(
            signature_validator
                .validate(&AgentIdentity::default(), &remote_config)
                .is_ok()
        )
    }
}
