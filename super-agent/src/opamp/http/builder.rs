use http::{HeaderMap, HeaderValue};
use std::sync::Arc;
use std::time::Duration;

use nr_auth::TokenRetriever;
use opamp_client::http::http_client::HttpClient;
use ureq::Agent;

use crate::super_agent::config::OpAMPClientConfig;

use super::client::HttpClientUreq;

/// Default client timeout is 30 seconds
const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(thiserror::Error, Debug)]
pub enum HttpClientBuilderError {
    #[error("`{0}`")]
    BuildingError(String),
}

pub trait HttpClientBuilder {
    type Client: HttpClient + Send + Sync + 'static;

    fn build(&self) -> Result<Self::Client, HttpClientBuilderError>;
}

#[derive(Debug, Clone)]
pub struct UreqHttpClientBuilder<T> {
    config: OpAMPClientConfig,
    token_retriever: Arc<T>,
}

impl<T> UreqHttpClientBuilder<T>
where
    T: TokenRetriever + Send + Sync + 'static,
{
    pub fn new(config: OpAMPClientConfig, token_retriever: Arc<T>) -> Self {
        Self {
            config,
            token_retriever,
        }
    }

    /// Return the headers from the configuration + the Content-Type header
    /// necessary for OpAMP (application/x-protobuf)
    fn headers(&self) -> HeaderMap {
        let mut headers = self.config.headers.clone();
        // Add headers for protobuf wire format communication
        headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/x-protobuf"),
        );
        headers
    }
}

impl<T> HttpClientBuilder for UreqHttpClientBuilder<T>
where
    T: TokenRetriever + Send + Sync + 'static,
{
    type Client = HttpClientUreq<T>;

    /// Build the HTTP Client. It will contain a Token Retriever, so in all
    /// post requests a Token will be retrieved from Identity System Service
    /// and injected as authorization header.
    fn build(&self) -> Result<Self::Client, HttpClientBuilderError> {
        let client = build_ureq_client();
        let url = self.config.endpoint.clone();
        let headers = self.headers();
        let token_retriever = self.token_retriever.clone();

        Ok(HttpClientUreq::new(client, url, headers, token_retriever))
    }
}

pub(super) fn build_ureq_client() -> Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(DEFAULT_CLIENT_TIMEOUT)
        .timeout(DEFAULT_CLIENT_TIMEOUT)
        .build()
}

#[cfg(test)]
pub(crate) mod test {
    use http::{HeaderMap, Response};
    use mockall::mock;
    use opamp_client::http::HttpClientError;
    use opamp_client::operation::settings::StartSettings;

    use crate::{
        event::channel::pub_sub,
        opamp::client_builder::{DefaultOpAMPClientBuilder, OpAMPClientBuilder},
        super_agent::config::AgentID,
    };

    use super::*;

    // Mock the HttpClient
    mock! {
        pub HttpClientMock {}
        impl HttpClient for HttpClientMock {
            fn post(&self, body: Vec<u8>) -> Result<Response<Vec<u8>>, HttpClientError>;
        }
    }

    // Mock the builder
    mock! {
        pub HttpClientBuilderMock {}
        impl HttpClientBuilder for HttpClientBuilderMock {
            type Client = MockHttpClientMock;
            fn build(&self) -> Result<MockHttpClientMock, HttpClientBuilderError>;
        }
    }

    #[test]
    fn test_default_http_client_builder() {
        let mut http_client = MockHttpClientMock::default();
        let mut http_builder = MockHttpClientBuilderMock::new();
        let opamp_config = OpAMPClientConfig {
            endpoint: "http://localhost".try_into().unwrap(),
            headers: HeaderMap::default(),
        };
        let (tx, _rx) = pub_sub();
        let agent_id = AgentID::new_super_agent_id();
        let start_settings = StartSettings::default();

        http_client // Define http client behavior for this test
            .expect_post()
            .times(1)
            .returning(|_| Ok(Response::new(vec![])));
        // Define http builder behavior for this test
        http_builder
            .expect_build()
            .times(1)
            .return_once(|| Ok(http_client));

        let builder = DefaultOpAMPClientBuilder::new(opamp_config, http_builder);
        let actual_client = builder.build_and_start(tx, agent_id, start_settings);

        assert!(actual_client.is_ok());
    }

    #[test]
    fn test_default_http_client_builder_error() {
        let mut http_builder = MockHttpClientBuilderMock::new();
        let opamp_config = OpAMPClientConfig {
            endpoint: "http://localhost".try_into().unwrap(),
            headers: HeaderMap::default(),
        };
        let (tx, _rx) = pub_sub();
        let agent_id = AgentID::new_super_agent_id();
        let start_settings = StartSettings::default();

        // Define http builder behavior for this test
        http_builder.expect_build().times(1).return_once(|| {
            Err(HttpClientBuilderError::BuildingError(String::from(
                "bad config",
            )))
        });

        let builder = DefaultOpAMPClientBuilder::new(opamp_config, http_builder);
        let actual_client = builder.build_and_start(tx, agent_id, start_settings);

        assert!(actual_client.is_err());

        let Err(err) = actual_client else {
            // I need to do this because this type doesn't implement Debug, TODO fix in opamp-rs!
            panic!("Expected an error");
        };
        assert_eq!(
            "error building http client: ``bad config``",
            err.to_string()
        );
    }
}
