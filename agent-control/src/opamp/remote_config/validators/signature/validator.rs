use crate::agent_control::defaults::get_custom_capabilities;
use crate::http::client::HttpClient;
use crate::http::config::HttpConfig;
use crate::http::config::ProxyConfig;
use crate::opamp::remote_config::RemoteConfig;
use crate::opamp::remote_config::signature::SIGNATURE_CUSTOM_CAPABILITY;
use crate::opamp::remote_config::validators::RemoteConfigValidator;
use crate::sub_agent::identity::AgentIdentity;
use nix::NixPath;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;
use tracing::log::error;
use tracing::{info, warn};
use url::Url;

use super::certificate_fetcher::CertificateFetcher;
use super::certificate_store::CertificateStore;

const DEFAULT_CERTIFICATE_SERVER_URL: &str = "https://newrelic.com/";
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
    let certificate_fetcher = if !config.certificate_pem_file_path.is_empty() {
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

    let certificate_store = CertificateStore::try_new(certificate_fetcher)
        .map_err(|e| SignatureValidatorError::BuildingValidator(e.to_string()))?;

    Ok(SignatureValidator::Validator(
        CertificateSignatureValidator::new(certificate_store),
    ))
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct SignatureValidatorConfig {
    #[serde(default = "default_signature_validator_url")]
    pub certificate_server_url: Url,
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
            certificate_server_url: default_signature_validator_url(),
            certificate_pem_file_path: PathBuf::new(),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct SignatureCertificateServerUrl(Url);

impl From<SignatureCertificateServerUrl> for Url {
    fn from(value: SignatureCertificateServerUrl) -> Self {
        value.0
    }
}

impl Default for SignatureCertificateServerUrl {
    fn default() -> Self {
        let certificate_server_url =  Url::parse(DEFAULT_CERTIFICATE_SERVER_URL).unwrap_or_else(
            |err| panic!("Invalid DEFAULT_CERTIFICATE_SERVER_URL: '{DEFAULT_CERTIFICATE_SERVER_URL}': {err}"));
        Self(certificate_server_url)
    }
}

fn default_signature_validator_url() -> Url {
    Url::parse(DEFAULT_CERTIFICATE_SERVER_URL).unwrap_or_else(|err| {
        panic!("Invalid DEFAULT_CERTIFICATE_SERVER_URL: '{DEFAULT_CERTIFICATE_SERVER_URL}': {err}")
    })
}

fn default_signature_validator_config_enabled() -> bool {
    DEFAULT_SIGNATURE_VALIDATOR_ENABLED
}

// NOTE: if we updated the components using the validator to use a composite-like implementation to handle validation,
// the no-op validator wouldn't be necessary.
/// The SignatureValidator enum wraps [CertificateSignatureValidator] and adds support for No-op validator.
pub enum SignatureValidator {
    Validator(CertificateSignatureValidator),
    Noop,
}

impl RemoteConfigValidator for SignatureValidator {
    type Err = SignatureValidatorError;

    fn validate(
        &self,
        agent_identity: &AgentIdentity,
        remote_config: &RemoteConfig,
    ) -> Result<(), Self::Err> {
        match self {
            SignatureValidator::Validator(v) => v.validate(agent_identity, remote_config),
            SignatureValidator::Noop => Ok(()),
        }
    }
}

/// The CertificateSignatureValidator is responsible for checking the validity of the signature.
pub struct CertificateSignatureValidator {
    certificate_store: CertificateStore,
}

impl CertificateSignatureValidator {
    pub fn new(certificate_store: CertificateStore) -> Self {
        Self { certificate_store }
    }
}

impl RemoteConfigValidator for CertificateSignatureValidator {
    type Err = SignatureValidatorError;

    fn validate(
        &self,
        agent_identity: &AgentIdentity,
        remote_config: &RemoteConfig,
    ) -> Result<(), SignatureValidatorError> {
        // custom capabilities are got from the agent-type (currently hard-coded)
        // If the capability is not set, no validation is performed
        if !get_custom_capabilities(&agent_identity.agent_type_id).is_some_and(|c| {
            c.capabilities
                .contains(&SIGNATURE_CUSTOM_CAPABILITY.to_string())
        }) {
            return Ok(());
        }

        let signature = remote_config
            .get_unique_signature()
            .map_err(|e| SignatureValidatorError::VerifySignature(e.to_string()))?
            .ok_or(SignatureValidatorError::VerifySignature(
                "Signature is missing".to_string(),
            ))?;

        let config_content = remote_config
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
mod tests {
    use std::collections::HashMap;
    use std::str::FromStr;

    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::opamp::remote_config::ConfigurationMap;
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::signature::{
        ECDSA_P256_SHA256, ED25519, SignatureData, Signatures,
    };
    use crate::opamp::remote_config::validators::signature::certificate_store::tests::TestSigner;
    use crate::sub_agent::identity::AgentIdentity;
    use assert_matches::assert_matches;

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
                    certificate_pem_file_path: PathBuf::from_str("/path/to/file").unwrap(),
                },
            },
            TestCase {
                name: "Setup file and url and enabled",
                cfg: r#"
enabled: true
certificate_server_url: https://example.com
certificate_pem_file_path: /path/to/file
"#,
                expected: SignatureValidatorConfig {
                    enabled: true,
                    certificate_server_url: Url::parse("https://example.com").unwrap(),
                    certificate_pem_file_path: PathBuf::from_str("/path/to/file").unwrap(),
                },
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_noop_signature_validator() {
        let rc = RemoteConfig::new(
            AgentID::new("test").unwrap(),
            Hash::new("test_payload".to_string()),
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
            remote_config: RemoteConfig,
        }

        impl TestCase {
            fn run(self) {
                let test_signer = TestSigner::new();

                let signature_validator = CertificateSignatureValidator::new(
                    CertificateStore::try_new(CertificateFetcher::PemFile(
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
                remote_config: RemoteConfig::new(
                    AgentID::new("test").unwrap(),
                    Hash::new("test_payload".to_string()),
                    None,
                ),
            },
            TestCase {
                name: "Signature cannot be retrieved because multiple signatures are defined",
                remote_config: RemoteConfig::new(
                    AgentID::new("test").unwrap(),
                    Hash::new("test_payload".to_string()),
                    None,
                )
                .with_signature(Signatures::new_multiple([
                    SignatureData::new("first", ED25519, "fake_key_id"),
                    SignatureData::new("second", ED25519, "fake_key_id"),
                ])),
            },
            TestCase {
                name: "Config is empty",
                remote_config: RemoteConfig::new(
                    AgentID::new("test").unwrap(),
                    Hash::new("test_payload".to_string()),
                    None,
                )
                .with_signature(Signatures::new_unique("", ED25519, "fake_key_id")),
            },
            TestCase {
                name: "Invalid signature",
                remote_config: RemoteConfig::new(
                    AgentID::new("test").unwrap(),
                    Hash::new("test_payload".to_string()),
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
        let test_signer = TestSigner::new();
        let signature_validator = CertificateSignatureValidator::new(
            CertificateStore::try_new(CertificateFetcher::PemFile(test_signer.cert_pem_path()))
                .unwrap(),
        );
        let rc = RemoteConfig::new(
            AgentID::new_agent_control_id(),
            Hash::new("test".to_string()),
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
        let test_signer = TestSigner::new();

        let signature_validator = CertificateSignatureValidator::new(
            CertificateStore::try_new(CertificateFetcher::PemFile(test_signer.cert_pem_path()))
                .unwrap(),
        );

        let config = "value";

        let encoded_signature = test_signer.encoded_signature(config);
        let remote_config = RemoteConfig::new(
            AgentID::new_agent_control_id(),
            Hash::new("test".to_string()),
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
