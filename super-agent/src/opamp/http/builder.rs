use opamp_client::http::{http_client::HttpClient, HttpClientError};

use crate::super_agent::config::OpAMPClientConfig;

use super::client::HttpClientUreq;

pub trait HttpClientBuilder {
    type Client: HttpClient + Send + Sync + 'static;

    fn build(&self) -> Result<Self::Client, HttpClientError>;
}

#[derive(Debug, Clone)]
pub struct DefaultHttpClientBuilder {
    config: OpAMPClientConfig,
}

impl DefaultHttpClientBuilder {
    pub fn new(config: OpAMPClientConfig) -> Self {
        Self { config }
    }
}

impl HttpClientBuilder for DefaultHttpClientBuilder {
    type Client = HttpClientUreq;
    fn build(&self) -> Result<Self::Client, HttpClientError> {
        Ok(HttpClientUreq::from(&self.config))
    }
}
