use std::time::Duration;
use crate::crowdstrike::response::Token;
use crate::http_client::{HttpClient, HttpClientError};

pub(crate) const TOKEN_HEADER: &str = "Authorization";

/// An implementation of the `HttpClient` trait using the ureq library and IMDv2 auth.
pub struct CrowdstrikeHttpClientUreq {
    http_client: ureq::Agent,
    client_id: String,
    client_secret: String,
    token_endpoint: String,
    installers_endpoint: String,
}

impl CrowdstrikeHttpClientUreq {
    pub fn new(
        client_id: String,
        client_secret: String,
        token_endpoint: String,
        installers_endpoint: String,
        timeout: Duration,
    ) -> Self {
        Self {
            http_client: ureq::AgentBuilder::new()
                .timeout_connect(timeout)
                .timeout(timeout)
                .build(),
            client_id,
            client_secret,
            token_endpoint,
            installers_endpoint,
        }
    }

    fn get_token(&self) -> Result<String, HttpClientError> {
        let response = self
            .http_client
            .post(self.token_endpoint.as_str())
            .send_form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
            ])?;

        let token: Token = response.into_json()?;
        Ok(format!("Bearer {}", token.access_token))
    }
}

impl HttpClient for CrowdstrikeHttpClientUreq {
    fn get(&self) -> Result<http::Response<Vec<u8>>, HttpClientError> {
        let token = self.get_token()?;

        let req = self
            .http_client
            .get(&self.installers_endpoint)
            .set(TOKEN_HEADER, &token);

        Ok(req.call()?.into())
    }
}
