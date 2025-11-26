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

        // Iterate over all remote configs and verify signatures
        for (config_name, config_content) in opamp_remote_config.configs_iter() {
            let signature = opamp_remote_config.signature(config_name).map_err(|e| {
                SignatureValidatorError::VerifySignature(format!(
                    "getting signature for config '{}' config signature: {}",
                    config_name, e
                ))
            })?;

            public_key_store
                .verify_signature(
                    signature.signature_algorithm(),
                    signature.key_id(),
                    config_content.as_bytes(),
                    signature.signature(),
                )
                .map_err(|e| {
                    SignatureValidatorError::VerifySignature(format!(
                        "verifying signature for config '{}': {}",
                        config_name, e
                    ))
                })?;
        }

        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::opamp::remote_config::ConfigurationMap;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::opamp::remote_config::signature::{
        ED25519, SignatureFields, Signatures, SigningAlgorithm,
    };
    use crate::opamp::remote_config::validators::signature::public_key_fetcher::tests::FakePubKeyServer;
    use crate::sub_agent::identity::AgentIdentity;
    use assert_matches::assert_matches;
    use std::collections::HashMap;

    const DEFAULT_CONFIG_KEY: &str = "test_key";

    #[rstest::rstest]
    #[case::single_valid_signature(
        AgentIdentity::default(),
        HashMap::from([("config1".to_string(), "value".to_string())])
    )]
    #[case::agent_control_single_valid_signature(
        AgentIdentity::new_agent_control_identity(),
        HashMap::from([("config1".to_string(), "value".to_string())])
    )]
    #[case::multiple_valid_signatures(
        AgentIdentity::default(),
        HashMap::from([
            ("config1".to_string(), "value1".to_string()),
            ("config2".to_string(), "value2".to_string()),
            ("config3".to_string(), "value3".to_string()),
        ])
    )]
    #[case::agent_control_multiple_valid_signatures(
        AgentIdentity::new_agent_control_identity(),
        HashMap::from([
            ("config1".to_string(), "value1".to_string()),
            ("config2".to_string(), "value2".to_string()),
            ("config3".to_string(), "value3".to_string()),
        ])
    )]
    pub fn test_valid_signature(
        #[case] agent_identity: AgentIdentity,
        #[case] configs: HashMap<String, String>,
    ) {
        let pub_key_server = FakePubKeyServer::new();

        let signature_validator = SignatureValidator::new(
            SignatureValidatorConfig {
                public_key_server_url: Some(pub_key_server.url.clone()),
                ..Default::default()
            },
            ProxyConfig::default(),
        )
        .unwrap();

        // Create signatures for all configs
        let mut signatures = Signatures {
            signatures: HashMap::new(),
        };
        for (config_name, config_content) in &configs {
            let encoded_signature = pub_key_server.sign(config_content.as_bytes());
            signatures.signatures.insert(
                config_name.clone(),
                SignatureFields {
                    signature: encoded_signature,
                    key_id: pub_key_server.key_id.clone(),
                    signing_algorithm: SigningAlgorithm::ED25519,
                },
            );
        }

        let remote_config = OpampRemoteConfig::new(
            agent_identity.id.clone(),
            Hash::from("test"),
            ConfigState::Applying,
            ConfigurationMap::new(configs),
        )
        .with_signature(signatures);

        signature_validator
            .validate(&agent_identity, &remote_config)
            .unwrap();
    }

    #[test]
    pub fn test_partial_valid() {
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

        let remote_config = OpampRemoteConfig::new(
            AgentIdentity::default().id,
            Hash::from("test"),
            ConfigState::Applying,
            ConfigurationMap::new(HashMap::from([
                (DEFAULT_CONFIG_KEY.to_string(), config.to_string()),
                (
                    "config-with-missing-signature".to_string(),
                    config.to_string(),
                ),
            ])),
        )
        .with_signature(Signatures::new_default(
            DEFAULT_CONFIG_KEY,
            encoded_signature.as_str(),
            ED25519,
            pub_key_server.key_id.as_str(),
        ));

        assert_matches!(
            signature_validator.validate(&AgentIdentity::default(), &remote_config),
            Err(SignatureValidatorError::VerifySignature(_))
        );
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

    #[rstest::rstest]
    #[case::all_signatures_missing(
        AgentIdentity::default(),
        OpampRemoteConfig::new(
            AgentID::try_from("test").unwrap(),
            Hash::from("test_payload"),
            ConfigState::Applying,
            ConfigurationMap::new(HashMap::from([
                ("key".to_string(), "value".to_string()),
                ("key2".to_string(), "value".to_string()),
            ])),
        )
    )]
    #[case::signature_missing(
        AgentIdentity::default(),
        OpampRemoteConfig::new(
            AgentID::try_from("test").unwrap(),
            Hash::from("test_payload"),
            ConfigState::Applying,
            ConfigurationMap::new(HashMap::from([(
                "some-key".to_string(),
                "value".to_string(),
            )])),
        )
        .with_signature(Signatures::new_default(DEFAULT_CONFIG_KEY,
            "invalid signature",
            ED25519,
            "fake_key_id",
        ))
    )]
    #[case::invalid_sub_agent_signature(
        AgentIdentity::default(),
        OpampRemoteConfig::new(
            AgentID::try_from("test").unwrap(),
            Hash::from("test_payload"),
            ConfigState::Applying,
            ConfigurationMap::new(HashMap::from([(
                DEFAULT_CONFIG_KEY.to_string(),
                "value".to_string(),
            )])),
        )
        .with_signature(Signatures::new_default(DEFAULT_CONFIG_KEY,
            "invalid signature",
            ED25519,
            "fake_key_id",
        ))
    )]
    #[case::missing_signature_for_agent_control(
        AgentIdentity::new_agent_control_identity(),
        OpampRemoteConfig::new(
            AgentID::AgentControl,
            Hash::from("test"),
            ConfigState::Applying,
            ConfigurationMap::new(HashMap::from([("key".to_string(), "value".to_string())])),
        )
    )]
    pub fn test_signature_validator_errors(
        #[case] agent_identity: AgentIdentity,
        #[case] remote_config: OpampRemoteConfig,
    ) {
        let pub_key_server = FakePubKeyServer::new();

        let signature_validator = SignatureValidator::new(
            SignatureValidatorConfig {
                public_key_server_url: Some(pub_key_server.url.clone()),
                ..Default::default()
            },
            ProxyConfig::default(),
        )
        .unwrap();

        assert_matches!(
            signature_validator.validate(&agent_identity, &remote_config),
            Err(SignatureValidatorError::VerifySignature(_))
        );
    }
}
