use crate::agent_control::config::AgentTypeFQN;
use crate::event::channel::{pub_sub, EventConsumer, EventPublisher};
use crate::opamp::remote_config::signature::SIGNATURE_CUSTOM_CAPABILITY;
use crate::opamp::remote_config::RemoteConfig;
use std::ops::Not;
use std::sync::{Arc, Mutex};
use std::thread::spawn;
use std::time::Duration;
use std::time::SystemTime;
use thiserror::Error;
use tracing::debug;
use tracing::log::error;

const DEFAULT_CERTIFICATE_TTL: Duration = Duration::from_secs(60);

type ErrorMessage = String;
#[derive(Error, Debug)]
pub enum SignatureValidatorError {
    #[error("failed to fetch certificate: `{0}`")]
    Certificate(ErrorMessage),
}

/// The SignatureValidator is responsible for checking the validity of the signature.
/// It tries to fetch the signature
pub struct SignatureValidator {
    last_updated_time: Mutex<SystemTime>,
    certificate: Mutex<Certificate>,
    stop_publisher: EventPublisher<()>,
}

// TODO This is just a stub
pub struct Certificate;

impl SignatureValidator {
    pub fn try_new() -> Result<Arc<Self>, SignatureValidatorError> {
        SignatureValidator::try_new_with_ttl(DEFAULT_CERTIFICATE_TTL)
    }

    fn try_new_with_ttl(certificate_ttl: Duration) -> Result<Arc<Self>, SignatureValidatorError> {
        let (stop_publisher, stop_consumer) = pub_sub();

        let signature_validator = Arc::new(SignatureValidator {
            certificate: Mutex::new(fetch_certificate()?),
            last_updated_time: Mutex::new(SystemTime::now()),
            stop_publisher,
        });

        fetch_certificate_periodically(signature_validator.clone(), stop_consumer, certificate_ttl);

        Ok(signature_validator)
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
                    // TODO right now there is no validation in place
                    return Ok(remote_config.get_signature().is_some());
                }
                Ok(true)
            }
        }
    }

    pub fn stop(&self) {
        _ = self.stop_publisher.publish(())
    }
}

fn fetch_certificate_periodically(
    certificate_data: Arc<SignatureValidator>,
    stop_consumer: EventConsumer<()>,
    ttl: Duration,
) {
    spawn(move || loop {
        if stop_consumer.as_ref().try_recv().is_ok() {
            debug!("Stopping Updating Certificate loop");
            return;
        }

        let mut certificate = certificate_data
            .certificate
            .lock()
            .expect("failed to acquire certificate lock");

        let mut last_updated_time = certificate_data
            .last_updated_time
            .lock()
            .expect("failed to acquire last_updated_time lock");

        _ = last_updated_time
            .elapsed()
            .inspect_err(|e| error!("old time greater than new time: {:?}", e))
            .map(|t| {
                if t > ttl {
                    debug!("Updating Certificate");
                    _ = fetch_certificate()
                        .inspect_err(|e| error!("error fetching certificate: {:?}", e))
                        .map(|c| {
                            *certificate = c;
                            *last_updated_time = SystemTime::now()
                        });
                }
            });
    });
}

//TODO this is a stub
fn fetch_certificate() -> Result<Certificate, SignatureValidatorError> {
    Ok(Certificate)
}

impl Drop for SignatureValidator {
    fn drop(&mut self) {
        self.stop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Duration};

    #[test]
    pub fn test_signature_validator_stops_routine() {
        let validator = SignatureValidator::try_new_with_ttl(Duration::from_millis(100)).unwrap();
        thread::sleep(Duration::from_secs(3));
        // We check that the certificate has been updated
        assert!(
            validator
                .last_updated_time
                .lock()
                .unwrap()
                .elapsed()
                .unwrap()
                < Duration::from_millis(200)
        );

        validator.stop();
        thread::sleep(Duration::from_secs(2));
        // We double-check that the certificate has been updated
        assert!(
            validator
                .last_updated_time
                .lock()
                .unwrap()
                .elapsed()
                .unwrap()
                > Duration::from_secs(1)
        );
    }
}
