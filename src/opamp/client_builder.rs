use futures::executor::block_on;
use opamp_client::error::{NotStartedClientError, StartedClientError};
use opamp_client::http::{
    HttpClientError, HttpClientReqwest, HttpConfig, NotStartedHttpClient, StartedHttpClient,
};
use opamp_client::opamp::proto::AgentHealth;
use opamp_client::operation::settings::StartSettings;
use opamp_client::{Client, NotStartedClient, StartedClient};
use std::time::SystemTimeError;
use thiserror::Error;
use tracing::error;

use crate::config::super_agent_configs::{AgentID, OpAMPClientConfig};

use crate::context::Context;
use crate::super_agent::callbacks::AgentCallbacks;
use crate::super_agent::super_agent::SuperAgentEvent;
use crate::utils::time::get_sys_time_nano;

#[derive(Error, Debug)]
pub enum OpAMPClientBuilderError {
    #[error("unable to create OpAMP HTTP client: `{0}`")]
    HttpClientError(#[from] HttpClientError),
    #[error("`{0}`")]
    NotStartedClientError(#[from] NotStartedClientError),
    #[error("`{0}`")]
    StartedClientError(#[from] StartedClientError),
    #[error("`{0}`")]
    StartedOpAMPlientError(#[from] opamp_client::error::ClientError),
    #[error("system time error: `{0}`")]
    SystemTimeError(#[from] SystemTimeError),
}

pub trait OpAMPClientBuilder {
    type Client: StartedClient;
    // type StartedClient: StartedClient;
    fn build_and_start(
        &self,
        ctx: Context<Option<SuperAgentEvent>>,
        agent_id: AgentID,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError>;
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
    type Client = StartedHttpClient<AgentCallbacks, HttpClientReqwest>;
    fn build_and_start(
        &self,
        ctx: Context<Option<SuperAgentEvent>>,
        agent_id: AgentID,
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

        let callbacks = AgentCallbacks::new(ctx, agent_id);

        let not_started_client = NotStartedHttpClient::new(callbacks, start_settings, http_client)?;

        let started_client = block_on(not_started_client.start())?;
        // set OpAMP health
        block_on(started_client.set_health(AgentHealth {
            healthy: true,
            start_time_unix_nano: get_sys_time_nano()?,
            last_error: "".to_string(),
        }))?;

        Ok(started_client)
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use async_trait::async_trait;
    use mockall::{mock, predicate};
    use opamp_client::error::ClientError;
    use opamp_client::operation::settings::StartSettings;
    use opamp_client::{
        error::{ClientResult, NotStartedClientResult, StartedClientResult},
        opamp::proto::{AgentDescription, AgentHealth, RemoteConfigStatus},
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

            async fn set_remote_config_status(&self, status: RemoteConfigStatus) -> ClientResult<()>;
        }
    }

    impl MockOpAMPClientMock {
        pub fn should_set_health(&mut self, times: usize) {
            self.expect_set_health().times(times).returning(|_| Ok(()));
        }
        #[allow(dead_code)]
        pub fn should_not_set_health(&mut self, times: usize, status_code: u16, error_msg: String) {
            self.expect_set_health().times(times).returning(move |_| {
                Err(ClientError::SenderError(
                    HttpClientError::UnsuccessfulResponse(status_code, error_msg.clone()),
                ))
            });
        }
        pub fn should_stop(&mut self, times: usize) {
            self.expect_stop().times(times).returning(|| Ok(()));
        }
        #[allow(dead_code)]
        pub fn should_not_stop(&mut self, times: usize, status_code: u16, error_msg: String) {
            self.expect_stop().times(times).returning(move || {
                Err(StartedClientError::SenderError(
                    HttpClientError::UnsuccessfulResponse(status_code, error_msg.clone()),
                ))
            });
        }
    }

    mock! {
        pub OpAMPClientBuilderMock {}

        impl OpAMPClientBuilder for OpAMPClientBuilderMock {
            type Client = MockOpAMPClientMock;
            fn build_and_start(&self, ctx: Context<Option<SuperAgentEvent>>, agent_id: AgentID, start_settings: StartSettings) -> Result<<Self as OpAMPClientBuilder>::Client, OpAMPClientBuilderError>;
        }
    }

    impl MockOpAMPClientBuilderMock {
        pub fn should_build_and_start<F>(
            &mut self,
            agent_id: AgentID,
            start_settings: StartSettings,
            returning: F,
        ) where
            F: FnMut(
                    Context<Option<SuperAgentEvent>>,
                    AgentID,
                    StartSettings,
                ) -> Result<MockOpAMPClientMock, OpAMPClientBuilderError>
                + Send
                + 'static,
        {
            self.expect_build_and_start()
                .with(
                    predicate::always(),
                    predicate::eq(agent_id),
                    predicate::eq(start_settings),
                )
                .once()
                .returning(returning);
        }
    }
}
