use crate::agent_control::config::AgentTypeFQN;
use crate::opamp::remote_config::signature::SIGNATURE_CUSTOM_CAPABILITY;
use crate::opamp::remote_config::validators::certificate_fetcher::{
    CertificateFetcher, DEFAULT_CERTIFICATE_TTL,
};
use crate::opamp::remote_config::RemoteConfig;
use std::ops::Not;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::log::error;

type ErrorMessage = String;
#[derive(Error, Debug)]
pub enum SignatureValidatorError {
    #[error("failed to fetch certificate: `{0}`")]
    FetchCertificate(ErrorMessage),
}

/// The SignatureValidator is responsible for checking the validity of the signature.
pub struct SignatureValidator {
    certificate_fetcher: CertificateFetcher,
}

// TODO This is just a stub
#[derive(Clone)]
pub struct Certificate;

impl SignatureValidator {
    pub fn new() -> Arc<Self> {
        SignatureValidator::new_with_ttl(DEFAULT_CERTIFICATE_TTL)
    }

    fn new_with_ttl(certificate_ttl: Duration) -> Arc<Self> {
        let certificate_fetcher = CertificateFetcher::new(certificate_ttl);

        Arc::new(SignatureValidator {
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
                    .not()
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
