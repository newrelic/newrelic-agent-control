use crate::http::client::HttpClient;
use crate::http::config::HttpConfig;
use crate::http::config::ProxyConfig;
use crate::opamp::remote_config::OpampRemoteConfig;
use crate::opamp::remote_config::validators::RemoteConfigValidator;
use crate::opamp::remote_config::validators::signature::public_key::PublicKey;
use crate::opamp::remote_config::validators::signature::public_key_fetcher::PublicKeyFetcher;
use crate::opamp::remote_config::validators::signature::verifier::VerifierStore;
use crate::sub_agent::identity::AgentIdentity;
use serde::Deserialize;
use std::time::Duration;
use thiserror::Error;
use tracing::info;
use tracing::log::error;
use tracing::warn;
use url::Url;

const DEFAULT_HTTPS_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_SIGNATURE_VALIDATOR_ENABLED: bool = true;

type ErrorMessage = String;
#[derive(Error, Debug)]
pub enum SignatureValidatorError {
    #[error("failed to build validator: {0}")]
    BuildingValidator(ErrorMessage),
    #[error("failed to verify signature: {0}")]
    VerifySignature(ErrorMessage),
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct SignatureValidatorConfig {
    #[serde(default = "default_signature_validator_config_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub public_key_server_url: Option<Url>,
}
impl Default for SignatureValidatorConfig {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_SIGNATURE_VALIDATOR_ENABLED,
            public_key_server_url: None,
        }
    }
}

fn default_signature_validator_config_enabled() -> bool {
    DEFAULT_SIGNATURE_VALIDATOR_ENABLED
}

pub struct SignatureValidator {
    public_key_store: Option<VerifierStore<PublicKey, PublicKeyFetcher>>,
}

impl SignatureValidator {
    pub fn new(
        config: SignatureValidatorConfig,
        proxy_config: ProxyConfig,
    ) -> Result<Self, SignatureValidatorError> {
        if !config.enabled {
            warn!("Remote config signature validation is disabled");
            return Ok(Self::new_noop());
        }

        let Some(public_key_server_url) = config.public_key_server_url else {
            return Err(SignatureValidatorError::BuildingValidator(
                "missing public_key_server_url configuration".to_string(),
            ));
        };

        info!(
            "Remote config signature validation is enabled, fetching jwks from: {}",
            public_key_server_url
        );

        let http_config = HttpConfig::new(
            DEFAULT_HTTPS_CLIENT_TIMEOUT,
            DEFAULT_HTTPS_CLIENT_TIMEOUT,
            proxy_config,
        );
        let http_client = HttpClient::new(http_config)
            .map_err(|e| SignatureValidatorError::BuildingValidator(e.to_string()))?;

        let public_key_fetcher = PublicKeyFetcher::new(http_client, public_key_server_url);

        let pubkey_verifier_store = VerifierStore::try_new(public_key_fetcher)
            .map_err(|err| SignatureValidatorError::BuildingValidator(err.to_string()))?;

        Ok(Self {
            public_key_store: Some(pubkey_verifier_store),
        })
    }

    pub fn new_noop() -> Self {
        Self {
            public_key_store: None,
        }
    }
}

impl RemoteConfigValidator for SignatureValidator {
    type Err = SignatureValidatorError;

    fn validate(
        &self,
        _: &AgentIdentity,
        opamp_remote_config: &OpampRemoteConfig,
    ) -> Result<(), Self::Err> {
        // Noop validation
        let Some(public_key_store) = &self.public_key_store else {
            return Ok(());
        };

        let signature = opamp_remote_config
            .get_default_signature()
            .map_err(|e| SignatureValidatorError::VerifySignature(e.to_string()))?
            .ok_or(SignatureValidatorError::VerifySignature(
                "Signature is missing".to_string(),
            ))?;

        let config_content = opamp_remote_config
            .get_default()
            .map_err(|e| SignatureValidatorError::VerifySignature(e.to_string()))?
            .as_bytes();

        public_key_store
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
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::opamp::remote_config::signature::{ED25519, SignatureData, Signatures};
    use crate::opamp::remote_config::validators::signature::public_key_fetcher::tests::FakePubKeyServer;
    use crate::opamp::remote_config::{ConfigurationMap, DEFAULT_AGENT_CONFIG_IDENTIFIER};
    use crate::sub_agent::identity::AgentIdentity;
    use assert_matches::assert_matches;
    use std::collections::HashMap;

    #[test]
    pub fn test_valid_signature() {
        let pub_key_server = FakePubKeyServer::new();

        let signature_validator = SignatureValidator::new(
            SignatureValidatorConfig {
                public_key_server_url: Some(pub_key_server.url.clone()),
                ..Default::default()
            },
            ProxyConfig::default(),
        )
        .unwrap();

        let config = "value";
        let encoded_signature = pub_key_server.sign(config.as_bytes());

        // agent remote config
        let remote_config = OpampRemoteConfig::new(
            AgentIdentity::default().id,
            Hash::from("test"),
            ConfigState::Applying,
            ConfigurationMap::new(HashMap::from([(
                DEFAULT_AGENT_CONFIG_IDENTIFIER.to_string(),
                config.to_string(),
            )])),
        )
        .with_signature(Signatures::new_default(
            encoded_signature.as_str(),
            ED25519,
            pub_key_server.key_id.as_str(),
        ));

        signature_validator
            .validate(&AgentIdentity::default(), &remote_config)
            .unwrap();

        // agent-control remote config
        let remote_config = OpampRemoteConfig::new(
            AgentIdentity::new_agent_control_identity().id,
            Hash::from("test"),
            ConfigState::Applying,
            ConfigurationMap::new(HashMap::from([(
                DEFAULT_AGENT_CONFIG_IDENTIFIER.to_string(),
                config.to_string(),
            )])),
        )
        .with_signature(Signatures::new_default(
            encoded_signature.as_str(),
            ED25519,
            pub_key_server.key_id.as_str(),
        ));

        signature_validator
            .validate(&AgentIdentity::new_agent_control_identity(), &remote_config)
            .unwrap()
    }

    #[test]
    fn test_noop_signature_validator() {
        let rc = OpampRemoteConfig::new(
            AgentID::try_from("test").unwrap(),
            Hash::from("test_payload"),
            ConfigState::Applying,
            ConfigurationMap::default(),
        );

        let noop_validator = SignatureValidator::new_noop();

        assert!(
            noop_validator
                .validate(&AgentIdentity::default(), &rc)
                .is_ok(),
            "The config should be valid even if the signature is missing when no-op validator is used",
        )
    }

    #[test]
    pub fn test_signature_validator_errors() {
        struct TestCase {
            name: &'static str,
            remote_config: OpampRemoteConfig,
        }

        impl TestCase {
            fn run(self) {
                let pub_key_server = FakePubKeyServer::new();

                let signature_validator = SignatureValidator::new(
                    SignatureValidatorConfig {
                        public_key_server_url: Some(pub_key_server.url.clone()),
                        ..Default::default()
                    },
                    ProxyConfig::default(),
                )
                .unwrap();

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
                    ConfigurationMap::default(),
                ),
            },
            TestCase {
                name: "Signature cannot be retrieved because multiple signatures are defined",
                remote_config: OpampRemoteConfig::new(
                    AgentID::try_from("test").unwrap(),
                    Hash::from("test_payload"),
                    ConfigState::Applying,
                    ConfigurationMap::default(),
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
                    ConfigurationMap::default(),
                )
                .with_signature(Signatures::new_default(
                    "",
                    ED25519,
                    "fake_key_id",
                )),
            },
            TestCase {
                name: "Invalid signature",
                remote_config: OpampRemoteConfig::new(
                    AgentID::try_from("test").unwrap(),
                    Hash::from("test_payload"),
                    ConfigState::Applying,
                    ConfigurationMap::new(HashMap::from([(
                        "key".to_string(),
                        "value".to_string(),
                    )])),
                )
                .with_signature(Signatures::new_default(
                    "invalid signature",
                    ED25519,
                    "fake_key_id",
                )),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    pub fn test_missing_signature_for_agent_control_agent() {
        let pub_key_server = FakePubKeyServer::new();

        let signature_validator = SignatureValidator::new(
            SignatureValidatorConfig {
                public_key_server_url: Some(pub_key_server.url.clone()),
                ..Default::default()
            },
            ProxyConfig::default(),
        )
        .unwrap();

        let rc = OpampRemoteConfig::new(
            AgentID::AgentControl,
            Hash::from("test"),
            ConfigState::Applying,
            ConfigurationMap::new(HashMap::from([("key".to_string(), "value".to_string())])),
        );

        assert_matches!(
            signature_validator.validate(&AgentIdentity::new_agent_control_identity(), &rc),
            Err(SignatureValidatorError::VerifySignature(_))
        );
    }
}
