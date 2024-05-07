use opamp_client::http::http_client::HttpClient;

use crate::super_agent::config::OpAMPClientConfig;

use super::client::HttpClientUreq;

pub trait HttpClientBuilder {
    type Client: HttpClient + Send + Sync + 'static;

    fn build(&self) -> Self::Client;
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
    fn build(&self) -> Self::Client {
        HttpClientUreq::from(&self.config)
    }
}
