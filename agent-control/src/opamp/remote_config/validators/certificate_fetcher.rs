use crate::opamp::remote_config::validators::signature::Certificate;
use std::sync::Mutex;
use std::time;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tracing::debug;
use tracing::log::error;

pub const DEFAULT_CERTIFICATE_TTL: Duration = Duration::from_secs(600);

type ErrorMessage = String;

#[derive(Error, Debug)]
pub enum CertificateFetchError {
    #[error("failed to compute elapsed time: `{0}`")]
    ElapsedTime(ErrorMessage),
}

/// The CertificateFetcher is responsible for retrieving when needed the certificate.
pub struct CertificateFetcher {
    last_updated_time: Mutex<SystemTime>,
    certificate: Mutex<Certificate>,
    ttl: Duration,
}

impl CertificateFetcher {
    pub fn new(ttl: Duration) -> Self {
        Self {
            last_updated_time: Mutex::new(time::UNIX_EPOCH),
            //TODO check how to initialise
            certificate: Mutex::new(Certificate),
            ttl,
        }
    }

    pub fn get_certificate(&self) -> Result<Certificate, CertificateFetchError> {
        let mut certificate = self
            .certificate
            .lock()
            .expect("failed to acquire certificate lock");

        let mut last_updated_time = self
            .last_updated_time
            .lock()
            .expect("failed to acquire last_updated_time lock");

        let time_difference = last_updated_time
            .elapsed()
            .map_err(|e| CertificateFetchError::ElapsedTime(e.to_string()))?;

        if time_difference > self.ttl {
            debug!("Updating Certificate");
            self.fetch_certificate()
                .inspect_err(|e| error!("error fetching certificate: {:?}", e))
                .map(|c| {
                    *certificate = c;
                    *last_updated_time = SystemTime::now();
                })?;
        }

        Ok(certificate.clone())
    }

    //TODO this is a stub
    fn fetch_certificate(&self) -> Result<Certificate, CertificateFetchError> {
        Ok(Certificate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Add;
    use std::time::Duration;

    #[test]
    pub fn test_certificate_fetcher_updates_time() {
        let validator = CertificateFetcher::new(Duration::from_millis(100));
        let _ = validator
            .get_certificate()
            .expect("failed to fetch certificate");
        assert!(
            validator
                .last_updated_time
                .lock()
                .unwrap()
                .elapsed()
                .unwrap()
                < Duration::from_millis(200)
        );
    }

    #[test]
    pub fn test_certificate_fetcher_fails() {
        let validator = CertificateFetcher::new(Duration::from_millis(100));
        let mut last_updated_time = validator.last_updated_time.lock().unwrap();
        *last_updated_time = SystemTime::now().add(Duration::from_secs(60));
        drop(last_updated_time);

        let result = validator.get_certificate();
        assert!(result.is_err());
    }

    #[test]
    pub fn test_certificate_is_not_updated() {
        let validator = CertificateFetcher::new(Duration::from_secs(15));

        let _ = validator.get_certificate().unwrap();
        let last_updated_time = validator.last_updated_time.lock().unwrap();
        let last_updated_time_clone = *last_updated_time;
        drop(last_updated_time);

        let _ = validator.get_certificate().unwrap();
        let _ = validator.get_certificate().unwrap();
        let new_last_updated_time = *validator.last_updated_time.lock().unwrap();

        assert_eq!(new_last_updated_time, last_updated_time_clone);
    }
}
