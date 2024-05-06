use opamp_client::http::http_client::HttpClient;

use crate::super_agent::config::OpAMPClientConfig;

use super::client::HttpClientUreq;

pub trait HttpClientBuilderT {
    fn build(self) -> impl HttpClient;
}

#[derive(Debug, Clone)]
pub struct DefaultHttpClientBuilder {
    config: OpAMPClientConfig,
}

impl DefaultHttpClientBuilder {
    pub fn new(config: &OpAMPClientConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

impl HttpClientBuilderT for DefaultHttpClientBuilder {
    fn build(self) -> impl HttpClient {
        HttpClientUreq::from(&self.config)
    }
}
