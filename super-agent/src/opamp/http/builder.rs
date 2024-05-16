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

#[cfg(test)]
pub(crate) mod test {
    use std::io;

    use crate::{
        event::channel::pub_sub,
        opamp::client_builder::{DefaultOpAMPClientBuilder, OpAMPClientBuilder},
        super_agent::config::AgentID,
    };

    use super::*;
    use http::{HeaderMap, Response};
    use mockall::mock;
    use opamp_client::operation::settings::StartSettings;

    // Mock the HttpClient
    mock! {
        pub HttpClientUreqMock {}
        impl HttpClient for HttpClientUreqMock {
            fn post(&self, body: Vec<u8>) -> Result<Response<Vec<u8>>, HttpClientError>;
        }
    }

    // Mock the builder
    mock! {
        pub HttpClientBuilderMock {}
        impl HttpClientBuilder for HttpClientBuilderMock {
            type Client = MockHttpClientUreqMock;
            fn build(&self) -> Result<MockHttpClientUreqMock, HttpClientError>;
        }
    }

    #[test]
    fn test_default_http_client_builder() {
        let mut http_client = MockHttpClientUreqMock::new();
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
            Err(HttpClientError::IOError(io::Error::new(
                io::ErrorKind::Other,
                "test",
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
            err.to_string(),
            "unable to create OpAMP HTTP client: ``test``"
        );
    }
}
