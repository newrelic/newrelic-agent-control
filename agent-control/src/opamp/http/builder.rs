use http::header::CONTENT_TYPE;
use http::{HeaderMap, HeaderValue};
use std::sync::Arc;
use std::time::Duration;
use tracing::error;

use crate::agent_control::config::OpAMPClientConfig;
use crate::http::client::{HttpBuildError, HttpClient};
use crate::http::config::HttpConfig;
use crate::http::config::ProxyConfig;
use crate::opamp::auth::token_retriever::TokenRetrieverImpl;
use crate::opamp::http::client::HttpOpAMPClient;
use crate::secret_retriever::OpampSecretRetriever;
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
pub struct OpAMPHttpClientBuilder<R> {
    opamp_config: OpAMPClientConfig,
    proxy_config: ProxyConfig,
    secret_retriever: Arc<R>,
}

impl<R> OpAMPHttpClientBuilder<R>
where
    R: OpampSecretRetriever,
{
    pub fn new(
        opamp_config: OpAMPClientConfig,
        proxy_config: ProxyConfig,
        secret_retriever: Arc<R>,
    ) -> Self {
        Self {
            opamp_config,
            proxy_config,
            secret_retriever,
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

impl<R> HttpClientBuilder for OpAMPHttpClientBuilder<R>
where
    R: OpampSecretRetriever,
{
    type Client = HttpOpAMPClient<TokenRetrieverImpl>;

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
        let token_retriever = TokenRetrieverImpl::try_build(
            self.opamp_config.clone().auth_config,
            self.secret_retriever.clone(),
            self.proxy_config.clone(),
        )
        .inspect_err(|err| error!("Could not build OpAMP's token retriever: {err}"))
        .map_err(|e| {
            HttpClientBuilderError::BuildingError(format!(
                "error trying to build OpAMP's token retriever: {e}"
            ))
        })?;

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
    use opamp_client::{StartedClient, http::HttpClientError};

    use crate::opamp::client_builder::{OpAMPClientBuilderError, PollInterval};
    use crate::opamp::instance_id::InstanceID;
    use crate::opamp::{
        client_builder::{OpAMPClientBuilder, OpAMPClientBuilderImpl},
        effective_config::loader::tests::{
            MockEffectiveConfigLoader, MockEffectiveConfigLoaderBuilder,
        },
    };
    use crate::sub_agent::identity::AgentIdentity;

    use super::*;

    // Mock the HttpClient
    mock! {
        pub HttpClient {}
        impl OpampHttpClient for HttpClient {
            fn post(&self, body: Vec<u8>) -> Result<Response<Vec<u8>>, HttpClientError>;
        }
    }

    // Mock the builder
    mock! {
        pub HttpClientBuilder {}
        impl HttpClientBuilder for HttpClientBuilder {
            type Client = MockHttpClient;
            fn build(&self) -> Result<MockHttpClient, HttpClientBuilderError>;
        }
    }

    #[test]
    fn test_default_http_client_builder() {
        let mut http_client = MockHttpClient::default();
        let mut http_builder = MockHttpClientBuilder::new();

        let mut effective_config_loader_builder = MockEffectiveConfigLoaderBuilder::new();
        effective_config_loader_builder
            .expect_build()
            .once()
            .return_once(|_| MockEffectiveConfigLoader::default());

        http_client // Define http client behavior for this test
            .expect_post()
            .times(1 + 1) // first message + drop
            .returning(|_| Ok(Response::new(vec![])));
        // Define http builder behavior for this test
        http_builder
            .expect_build()
            .once()
            .return_once(|| Ok(http_client));

        let builder = OpAMPClientBuilderImpl::new(
            PollInterval::default(),
            Arc::new(http_builder),
            Arc::new(effective_config_loader_builder),
            InstanceID::create(),
        )
        .with_agent_identity(AgentIdentity::new_agent_control_identity());

        let (started_client, _consumer) = builder.build_and_start().unwrap();

        // gracefully shutdown the all threads to avoid mocks panicking go unnoticed
        started_client.stop().unwrap();
    }

    #[test]
    fn test_default_http_client_builder_error() {
        let mut http_builder = MockHttpClientBuilder::new();
        let mut effective_config_loader_builder = MockEffectiveConfigLoaderBuilder::new();
        effective_config_loader_builder.expect_build().never();

        // Define http builder behavior for this test
        http_builder.expect_build().once().return_once(|| {
            Err(HttpClientBuilderError::BuildingError(String::from(
                "bad config",
            )))
        });

        let builder = OpAMPClientBuilderImpl::new(
            PollInterval::default(),
            Arc::new(http_builder),
            Arc::new(effective_config_loader_builder),
            InstanceID::create(),
        )
        .with_agent_identity(AgentIdentity::new_agent_control_identity());
        let actual_client = builder.build_and_start();

        assert!(actual_client.is_err());

        let Err(err) = actual_client else {
            // I need to do this because this type doesn't implement Debug, TODO fix in opamp-rs!
            panic!("Expected an error");
        };
        assert_matches!(err, OpAMPClientBuilderError::HttpClientBuilderError(_));
    }
}
