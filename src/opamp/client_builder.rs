use opamp_client::error::{NotStartedClientError, StartedClientError};
use opamp_client::http::{HttpClientError, HttpClientReqwest, HttpConfig};
use opamp_client::operation::settings::StartSettings;
use opamp_client::StartedClient;
use std::time::SystemTimeError;
use thiserror::Error;
use tracing::error;

use crate::config::super_agent_configs::{AgentID, OpAMPClientConfig};

use crate::context::Context;
use crate::super_agent::super_agent::SuperAgentEvent;

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

pub fn build_http_client(
    config: &OpAMPClientConfig,
) -> Result<HttpClientReqwest, OpAMPClientBuilderError> {
    let headers = config.headers.clone().unwrap_or_default();
    let headers: Vec<(&str, &str)> = headers
        .iter()
        .map(|(h, v)| (h.as_str(), v.as_str()))
        .collect();

    let http_client =
        HttpClientReqwest::new(HttpConfig::new(config.endpoint.as_str())?.with_headers(headers)?)?;

    Ok(http_client)
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

        // assertion just for the call of the method but not the remote
        // status itself (so any remote config status)
        pub fn should_set_any_remote_config_status(&mut self, times: usize) {
            self.expect_set_remote_config_status()
                .times(times)
                .returning(|_| Ok(()));
        }

        // assertion just for the call of the method but not the remote
        // status itself (so any remote config status)
        pub fn should_set_remote_config_status(&mut self, status: RemoteConfigStatus) {
            self.expect_set_remote_config_status()
                .once()
                .with(predicate::eq(status))
                .returning(|_| Ok(()));
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
