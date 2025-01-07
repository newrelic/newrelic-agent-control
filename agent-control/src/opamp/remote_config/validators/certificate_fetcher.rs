use crate::opamp::remote_config::validators::signature::Certificate;
use std::sync::Mutex;
use thiserror::Error;
use tracing::debug;
use tracing::log::error;

#[derive(Error, Debug)]
pub enum CertificateFetchError {}

/// The CertificateFetcher is responsible for returning the certificate.
pub struct CertificateFetcher {
    certificate: Mutex<Certificate>,
}

impl CertificateFetcher {
    pub fn try_new() -> Result<Self, CertificateFetchError> {
        CertificateFetcher::fetch_certificate().map(|certificate| Self {
            certificate: Mutex::new(certificate),
        })
    }

    pub fn get_certificate(&self) -> Result<Certificate, CertificateFetchError> {
        let mut certificate = self
            .certificate
            .lock()
            .expect("failed to acquire certificate lock");

        debug!("Updating Certificate");
        CertificateFetcher::fetch_certificate()
            .inspect_err(|e| error!("error fetching certificate: {:?}", e))
            .map(|c| {
                *certificate = c;
            })?;

        Ok(certificate.clone())
    }

    //TODO this is a stub
    fn fetch_certificate() -> Result<Certificate, CertificateFetchError> {
        Ok(Certificate)
    }
}
