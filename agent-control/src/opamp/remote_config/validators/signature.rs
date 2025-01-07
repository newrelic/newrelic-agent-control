use crate::agent_control::config::AgentTypeFQN;
use crate::opamp::remote_config::signature::SIGNATURE_CUSTOM_CAPABILITY;
use crate::opamp::remote_config::validators::certificate_fetcher::CertificateFetcher;
use crate::opamp::remote_config::RemoteConfig;
use thiserror::Error;
use tracing::log::error;

type ErrorMessage = String;
#[derive(Error, Debug)]
pub enum SignatureValidatorError {
    #[error("failed to fetch certificate: `{0}`")]
    FetchCertificate(ErrorMessage),
    #[error("failed to initialize the certificate fetcher: `{0}`")]
    InitialiseCertificateFetcher(ErrorMessage),
}

/// The SignatureValidator is responsible for checking the validity of the signature.
pub struct SignatureValidator {
    certificate_fetcher: CertificateFetcher,
}

// TODO This is just a stub
#[derive(Clone)]
pub struct Certificate;

impl SignatureValidator {
    pub fn try_new() -> Result<Self, SignatureValidatorError> {
        CertificateFetcher::try_new()
            .map_err(|e| SignatureValidatorError::InitialiseCertificateFetcher(e.to_string()))
            .map(|certificate_fetcher| Self {
                certificate_fetcher,
            })
    }

    pub fn validate(
        &self,
        agent_type_fqn: &AgentTypeFQN,
        remote_config: &RemoteConfig,
    ) -> Result<bool, SignatureValidatorError> {
        // TODO we are getting custom capabilities from the agentType, not the actual agent instance
        match agent_type_fqn.get_custom_capabilities() {
            None => Ok(true),
            Some(c) => {
                if c.capabilities
                    .contains(&SIGNATURE_CUSTOM_CAPABILITY.to_string())
                {
                    // TODO right now we are not using the certificate
                    _ = self
                        .certificate_fetcher
                        .get_certificate()
                        .map_err(|e| SignatureValidatorError::FetchCertificate(e.to_string()));
                    // TODO right now there is no validation in place
                    return Ok(remote_config.get_signature().is_some());
                }
                Ok(true)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::config::AgentID;
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::signature::Signature;
    use std::ops::Not;

    #[test]
    pub fn test_signature_is_missing() {
        let signature_validator = SignatureValidator::try_new().unwrap();
        let rc = RemoteConfig::new(
            AgentID::new("test").unwrap(),
            Hash::new("test_payload".to_string()),
            None,
        );
        let agent_type = AgentTypeFQN::try_from("ns/aa:1.1.3").unwrap();

        assert!(signature_validator
            .validate(&agent_type, &rc)
            .unwrap()
            .not());
    }

    #[test]
    pub fn test_signature_is_there() {
        let signature_validator = SignatureValidator::try_new().unwrap();
        let rc = RemoteConfig::new(
            AgentID::new("test").unwrap(),
            Hash::new("test_payload".to_string()),
            None,
        )
        .with_signature(Signature::new("test_signature".into()));
        let agent_type = AgentTypeFQN::try_from("ns/aa:1.1.3").unwrap();

        assert!(signature_validator.validate(&agent_type, &rc).unwrap());
    }

    #[test]
    pub fn test_signature_is_missing_for_agent_control() {
        let signature_validator = SignatureValidator::try_new().unwrap();
        let rc = RemoteConfig::new(
            AgentID::new_agent_control_id(),
            Hash::new("test".to_string()),
            None,
        );
        let agent_type = AgentTypeFQN::new_agent_control_fqn();

        assert!(signature_validator.validate(&agent_type, &rc).unwrap());
    }
}
