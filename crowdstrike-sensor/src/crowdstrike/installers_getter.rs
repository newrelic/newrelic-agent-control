use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;
use crate::crowdstrike::http_client::CrowdstrikeHttpClientUreq;
use crate::crowdstrike::response::Sensor;
use crate::http_client::{DEFAULT_CLIENT_TIMEOUT, HttpClient, HttpClientError};

/// The api endpoint to retrieve the token.
pub const CROWDSTRIKE_TOKEN_ENDPOINT: &str = "https://api.laggar.gcw.crowdstrike.com/oauth2/token";


/// The api endpoint to retrieve the sensor installers
pub const CROWDSTRIKE_INSTALLERS_ENDPOINT: &str = "https://api.laggar.gcw.crowdstrike.com/sensors/combined/installers/v1";

/// The `AWSDetector` struct encapsulates an HTTP client used to retrieve the instance metadata.
pub struct InstallerGetter<C: HttpClient> {
    http_client: C,
}

impl InstallerGetter<CrowdstrikeHttpClientUreq> {
    /// Returns a new instance of AWSDetector
    pub fn new(client_id: String, client_secret: String, token_endpoint: String, installers_endpoint: String) -> Self {
        Self {
            http_client: CrowdstrikeHttpClientUreq::new(
                client_id,
                client_secret,
                token_endpoint,
                installers_endpoint,
                DEFAULT_CLIENT_TIMEOUT,
            ),
        }
    }
}

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum InstallerGetterError {
    /// Internal HTTP error
    #[error("`{0}`")]
    HttpError(#[from] HttpClientError),
    /// Error while deserializing endpoint metadata
    #[error("`{0}`")]
    JsonError(#[from] serde_json::Error),
    /// Unsuccessful HTTP response.
    #[error("Status code: `{0}` Canonical reason: `{1}`")]
    UnsuccessfulResponse(u16, String),
}

impl<C> InstallerGetter<C>
where
    C: HttpClient,
{
    pub fn installers(&self) -> Result<HashMap<String, String>, InstallerGetterError> {
        let response = self
            .http_client
            .get()
            .map_err(InstallerGetterError::HttpError)?;

        let installers_response: Sensor =
            serde_json::from_slice(response.body()).map_err(InstallerGetterError::JsonError)?;

        let installers: HashMap<String, String> = installers_response.
            installers
            .iter()
            .map(|sensor_installer| {
                (sensor_installer.os.clone(), sensor_installer.sha256.clone())
            })
            .collect();

        Ok(installers)
    }
}