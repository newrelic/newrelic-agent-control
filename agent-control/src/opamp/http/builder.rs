use http::header::CONTENT_TYPE;
use http::{HeaderMap, HeaderValue};
use std::sync::Arc;
use std::time::Duration;

use super::client::HttpOpAMPClient;
use crate::agent_control::config::OpAMPClientConfig;
use crate::http::client::{HttpBuildError, HttpClient};
use crate::http::config::HttpConfig;
use crate::http::config::ProxyConfig;
use nr_auth::TokenRetriever;
use opamp_client::http::http_client::HttpClient as OpampHttpClient;

/// Default client timeout is 30 seconds
const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(thiserror::Error, Debug)]
pub enum HttpClientBuilderError {
    #[error("error building the OpAMP HTTP client: {0}")]
    BuildingError(String),
}

pub trait HttpClientBuilder {
    type Client: OpampHttpClient + Send + Sync + 'static;

    fn build(&self) -> Result<Self::Client, HttpClientBuilderError>;
}

#[derive(Debug, Clone)]
pub struct OpAMPHttpClientBuilder<T> {
    opamp_config: OpAMPClientConfig,
    proxy_config: ProxyConfig,
    token_retriever: Arc<T>,
}

impl<T> OpAMPHttpClientBuilder<T>
where
    T: TokenRetriever + Send + Sync + 'static,
{
    pub fn new(
        opamp_config: OpAMPClientConfig,
        proxy_config: ProxyConfig,
        token_retriever: Arc<T>,
    ) -> Self {
        Self {
            opamp_config,
            proxy_config,
            token_retriever,
        }
    }

    /// Return the headers from the configuration + the Content-Type header
    /// necessary for OpAMP (application/x-protobuf)
    fn headers(&self) -> HeaderMap {
        let mut headers = self.opamp_config.headers.clone();
        // Add headers for protobuf wire format communication
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-protobuf"),
        );
        headers
    }
}

impl<T> HttpClientBuilder for OpAMPHttpClientBuilder<T>
where
    T: TokenRetriever + Send + Sync + 'static,
{
    type Client = HttpOpAMPClient<T>;

    /// Build the HTTP Client. It will contain a Token Retriever, so in all
    /// post requests a Token will be retrieved from Identity System Service
    /// and injected as authorization header.
    fn build(&self) -> Result<Self::Client, HttpClientBuilderError> {
        let http_config = HttpConfig::new(
            DEFAULT_CLIENT_TIMEOUT,
            DEFAULT_CLIENT_TIMEOUT,
            self.proxy_config.clone(),
        );
        let url = self.opamp_config.endpoint.clone();
        let headers = self.headers();
        let client = HttpClient::new(http_config)?;
        let token_retriever = self.token_retriever.clone();

        Ok(HttpOpAMPClient::new(client, url, headers, token_retriever))
    }
}

impl From<HttpBuildError> for HttpClientBuilderError {
    fn from(err: HttpBuildError) -> Self {
        Self::BuildingError(err.to_string())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use assert_matches::assert_matches;
    use http::Response;
    use mockall::mock;
    use opamp_client::operation::settings::StartSettings;
    use opamp_client::{http::HttpClientError, StartedClient};

    use crate::opamp::client_builder::OpAMPClientBuilderError;
    use crate::{
        agent_control::config::AgentID,
        event::channel::pub_sub,
        opamp::{
            client_builder::{
                DefaultOpAMPClientBuilder, OpAMPClientBuilder, DEFAULT_POLL_INTERVAL,
            },
            effective_config::loader::tests::{
                MockEffectiveConfigLoaderBuilderMock, MockEffectiveConfigLoaderMock,
            },
        },
    };

    use super::*;

    // Mock the HttpClient
    mock! {
        pub HttpClientMock {}
        impl OpampHttpClient for HttpClientMock {
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
        let (tx, _rx) = pub_sub();
        let agent_id = AgentID::new_agent_control_id();
        let start_settings = StartSettings::default();

        let mut effective_config_loader_builder = MockEffectiveConfigLoaderBuilderMock::new();
        effective_config_loader_builder
            .expect_build()
            .once()
            .return_once(|_| MockEffectiveConfigLoaderMock::default());

        http_client // Define http client behavior for this test
            .expect_post()
            .times(1 + 1) // first message + drop
            .returning(|_| Ok(Response::new(vec![])));
        // Define http builder behavior for this test
        http_builder
            .expect_build()
            .once()
            .return_once(|| Ok(http_client));

        let builder = DefaultOpAMPClientBuilder::new(
            http_builder,
            effective_config_loader_builder,
            DEFAULT_POLL_INTERVAL,
        );

        let started_client = builder
            .build_and_start(tx, agent_id, start_settings)
            .unwrap();

        // gracefully shutdown the all threads to avoid mocks panicking go unnoticed
        started_client.stop().unwrap();
    }

    #[test]
    fn test_default_http_client_builder_error() {
        let mut http_builder = MockHttpClientBuilderMock::new();
        let (tx, _rx) = pub_sub();
        let agent_id = AgentID::new_agent_control_id();
        let start_settings = StartSettings::default();

        let mut effective_config_loader_builder = MockEffectiveConfigLoaderBuilderMock::new();
        effective_config_loader_builder.expect_build().never();

        // Define http builder behavior for this test
        http_builder.expect_build().once().return_once(|| {
            Err(HttpClientBuilderError::BuildingError(String::from(
                "bad config",
            )))
        });

        let builder = DefaultOpAMPClientBuilder::new(
            http_builder,
            effective_config_loader_builder,
            DEFAULT_POLL_INTERVAL,
        );
        let actual_client = builder.build_and_start(tx, agent_id, start_settings);

        assert!(actual_client.is_err());

        let Err(err) = actual_client else {
            // I need to do this because this type doesn't implement Debug, TODO fix in opamp-rs!
            panic!("Expected an error");
        };
        assert_matches!(err, OpAMPClientBuilderError::HttpClientBuilderError(_));
    }
}
