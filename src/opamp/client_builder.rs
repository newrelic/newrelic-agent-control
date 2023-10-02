use opamp_client::error::NotStartedClientError;
use opamp_client::http::{HttpClientError, HttpClientReqwest, HttpConfig, NotStartedHttpClient};
use opamp_client::operation::settings::StartSettings;
use opamp_client::NotStartedClient;
use thiserror::Error;
use tracing::error;

use crate::config::agent_configs::OpAMPClientConfig;

use crate::agent::callbacks::AgentCallbacks;

#[derive(Error, Debug)]
pub enum OpAMPClientBuilderError {
    #[error("unable to create OpAMP HTTP client: `{0}`")]
    HttpClientError(#[from] HttpClientError),
    #[error("`{0}`")]
    ClientError(#[from] NotStartedClientError),
}

pub trait OpAMPClientBuilder {
    type Client: NotStartedClient;
    fn build(&self, start_settings: StartSettings)
        -> Result<Self::Client, OpAMPClientBuilderError>;
}

/// OpAMPBuilderCfg
pub struct OpAMPHttpBuilder {
    config: OpAMPClientConfig,
}

impl OpAMPHttpBuilder {
    pub fn new(config: OpAMPClientConfig) -> Self {
        Self { config }
    }
}

impl OpAMPClientBuilder for OpAMPHttpBuilder {
    type Client = NotStartedHttpClient<AgentCallbacks, HttpClientReqwest>;
    fn build(
        &self,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError> {
        // TODO: cleanup
        let headers = self.config.headers.clone().unwrap_or_default();
        let headers: Vec<(&str, &str)> = headers
            .iter()
            .map(|header| (header.0.as_str(), header.1.as_str()))
            .collect();

        let http_client = HttpClientReqwest::new(
            HttpConfig::new(self.config.endpoint.as_str())?.with_headers(headers)?,
        )?;

        Ok(NotStartedHttpClient::new(
            AgentCallbacks,
            start_settings,
            http_client,
        )?)
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use async_trait::async_trait;
    use mockall::mock;
    use opamp_client::{
        error::{ClientResult, NotStartedClientResult, StartedClientResult},
        opamp::proto::{AgentDescription, AgentHealth},
        Client, NotStartedClient, StartedClient,
    };

    mock! {
        pub OpAMPClientMock {}

        #[async_trait]
        impl NotStartedClient for OpAMPClientMock {
            type StartedClient = MockOpAMPClientMock;
            // add code here
            async fn start(self) -> NotStartedClientResult<<Self as NotStartedClient>::StartedClient>;
        }

        #[async_trait]
        impl StartedClient for OpAMPClientMock {

            async fn stop(self) -> StartedClientResult<()>;
        }

        #[async_trait]
        impl Client for OpAMPClientMock {

            async fn set_agent_description(
                &self,
                description: AgentDescription,
            ) -> ClientResult<()>;

            async fn set_health(&self, health: AgentHealth) -> ClientResult<()>;

            async fn update_effective_config(&self) -> ClientResult<()>;
        }
    }

    mock! {
        pub OpAMPClientBuilderMock {}

        impl OpAMPClientBuilder for OpAMPClientBuilderMock {
            type Client = MockOpAMPClientMock;

            fn build(&self, start_settings: opamp_client::operation::settings::StartSettings) -> Result<<Self as OpAMPClientBuilder>::Client, OpAMPClientBuilderError>;
        }
    }
}
