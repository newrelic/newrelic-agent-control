use std::time::SystemTimeError;

use opamp_client::http::config::HttpConfigError;
use opamp_client::http::{HttpClientError, NotStartedHttpClient, StartedHttpClient};
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::operation::settings::StartSettings;
use opamp_client::{NotStartedClient, NotStartedClientError, StartedClient, StartedClientError};
use thiserror::Error;
use tracing::{error, info};

use crate::event::channel::EventPublisher;
use crate::event::OpAMPEvent;
use crate::opamp::instance_id;
use crate::super_agent::config::{AgentID, OpAMPClientConfig};

use super::callbacks::AgentCallbacks;
use super::http::builder::{HttpClientBuilder, HttpClientBuilderError};

#[derive(Error, Debug)]
pub enum OpAMPClientBuilderError {
    #[error("unable to create OpAMP HTTP client: `{0}`")]
    HttpClientError(#[from] HttpClientError),
    #[error("invalid HTTP configuration: `{0}`")]
    HttpConfigError(#[from] HttpConfigError),
    #[error("`{0}`")]
    NotStartedClientError(#[from] NotStartedClientError),
    #[error("`{0}`")]
    StartedClientError(#[from] StartedClientError),
    #[error("`{0}`")]
    StartedOpAMPlientError(#[from] opamp_client::ClientError),
    #[error("system time error: `{0}`")]
    SystemTimeError(#[from] SystemTimeError),
    #[error("error getting agent ulid: `{0}`")]
    GetUlidError(#[from] instance_id::GetterError),
    #[error("error building http client: `{0}`")]
    HttpClientBuilderError(#[from] HttpClientBuilderError),
}

pub trait OpAMPClientBuilder<CB>
where
    CB: Callbacks,
{
    type Client: StartedClient<CB> + 'static;
    fn build_and_start(
        &self,
        opamp_publisher: EventPublisher<OpAMPEvent>,
        agent_id: AgentID,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError>;
}

pub struct DefaultOpAMPClientBuilder<C> {
    config: OpAMPClientConfig,
    http_client_builder: C,
}

impl<C> DefaultOpAMPClientBuilder<C>
where
    C: HttpClientBuilder,
{
    pub fn new(config: OpAMPClientConfig, http_client_builder: C) -> Self {
        Self {
            config,
            http_client_builder,
        }
    }

    pub fn config(&self) -> &OpAMPClientConfig {
        &self.config
    }
}

impl<C> OpAMPClientBuilder<AgentCallbacks> for DefaultOpAMPClientBuilder<C>
where
    C: HttpClientBuilder,
{
    type Client = StartedHttpClient<AgentCallbacks, C::Client>;
    fn build_and_start(
        &self,
        opamp_publisher: EventPublisher<OpAMPEvent>,
        agent_id: AgentID,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError> {
        let http_client = self.http_client_builder.build()?;
        let callbacks = AgentCallbacks::new(agent_id, opamp_publisher);
        let not_started_client = NotStartedHttpClient::new(http_client);
        let started_client = not_started_client.start(callbacks, start_settings)?;
        info!("Super Agent OpAMP client started");
        Ok(started_client)
    }
}

#[cfg(test)]
pub(crate) mod test {
    use mockall::{mock, predicate};
    use opamp_client::operation::settings::StartSettings;
    use opamp_client::ClientError;
    use opamp_client::{
        opamp::proto::{AgentDescription, ComponentHealth, RemoteConfigStatus},
        Client, ClientResult, NotStartedClient, NotStartedClientResult, StartedClient,
        StartedClientResult,
    };

    use super::*;

    mock! {
        pub NotStartedOpAMPClientMock {}
        impl NotStartedClient for NotStartedOpAMPClientMock
         {
            type StartedClient<C: Callbacks + Send + Sync + 'static> = MockStartedOpAMPClientMock<C>;
            fn start<C: Callbacks + Send + Sync + 'static>(self, callbacks: C, start_settings: StartSettings ) -> NotStartedClientResult<<Self as NotStartedClient>::StartedClient<C>>;
        }
    }

    mock! {
        pub StartedOpAMPClientMock<C> where C: Callbacks {}

        impl<C> StartedClient<C> for StartedOpAMPClientMock<C>
            where
            C: Callbacks + Send + Sync + 'static {

            fn stop(self) -> StartedClientResult<()>;
        }

        impl<C> Client for StartedOpAMPClientMock<C>
        where
        C: Callbacks + Send + Sync + 'static {

             fn set_agent_description(
                &self,
                description: AgentDescription,
            ) -> ClientResult<()>;

             fn set_health(&self, health: ComponentHealth) -> ClientResult<()>;

             fn update_effective_config(&self) -> ClientResult<()>;

             fn set_remote_config_status(&self, status: RemoteConfigStatus) -> ClientResult<()>;
        }
    }

    impl<C> MockStartedOpAMPClientMock<C>
    where
        C: Callbacks + Send + Sync + 'static,
    {
        pub fn should_set_health(&mut self, times: usize) {
            self.expect_set_health().times(times).returning(|_| Ok(()));
        }

        pub fn should_set_healthy(&mut self) {
            self.expect_set_health()
                .withf(|health| health.healthy)
                .returning(|_| Ok(()));
        }

        pub fn should_set_unhealthy(&mut self) {
            self.expect_set_health()
                .withf(|health| !health.healthy)
                .returning(|_| Ok(()));
        }

        pub fn should_stop(&mut self, times: usize) {
            self.expect_stop().times(times).returning(|| Ok(()));
        }

        #[allow(dead_code)]
        pub fn should_not_stop(&mut self, times: usize, status_code: u16, error_msg: String) {
            self.expect_stop().times(times).returning(move || {
                Err(StartedClientError::SyncClientError(
                    ClientError::SenderError(HttpClientError::UnsuccessfulResponse(
                        status_code,
                        error_msg.clone(),
                    )),
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
        pub OpAMPClientBuilderMock<C> where C: Callbacks + Send + Sync + 'static{}

        impl<C> OpAMPClientBuilder<C> for OpAMPClientBuilderMock<C> where C: Callbacks + Send + Sync + 'static{
            type Client = MockStartedOpAMPClientMock<C>;
            fn build_and_start(&self, opamp_publisher: EventPublisher<OpAMPEvent>, agent_id: AgentID, start_settings: StartSettings) -> Result<<Self as OpAMPClientBuilder<C>>::Client, OpAMPClientBuilderError>;
        }
    }

    impl<C> MockOpAMPClientBuilderMock<C>
    where
        C: Callbacks + Send + Sync + 'static,
    {
        pub fn should_build_and_start(
            &mut self,
            agent_id: AgentID,
            start_settings: StartSettings,
            client: MockStartedOpAMPClientMock<C>,
        ) {
            self.expect_build_and_start()
                .with(
                    predicate::always(),
                    predicate::eq(agent_id),
                    predicate::eq(start_settings),
                )
                .once()
                .return_once(move |_, _, _| Ok(client));
        }
    }
}
