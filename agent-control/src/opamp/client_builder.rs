use super::callbacks::AgentCallbacks;
use super::effective_config::loader::EffectiveConfigLoaderBuilder;
use super::http::builder::{HttpClientBuilder, HttpClientBuilderError};
use crate::agent_control::agent_id::AgentID;
use crate::event::channel::EventPublisher;
use crate::event::OpAMPEvent;
use crate::opamp::instance_id;
use opamp_client::http::client::OpAMPHttpClient;
use opamp_client::http::{NotStartedHttpClient, StartedHttpClient};
use opamp_client::operation::settings::StartSettings;
use opamp_client::{NotStartedClient, NotStartedClientError, StartedClient};
use std::time::Duration;
use thiserror::Error;
use tracing::{error, info};

/// Default poll interval for the OpAMP http managed client
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Error, Debug)]
pub enum OpAMPClientBuilderError {
    #[error("OpAMP client: `{0}`")]
    NotStartedClientError(#[from] NotStartedClientError),
    #[error("error getting agent instance id: `{0}`")]
    GetInstanceIDError(#[from] instance_id::GetterError),
    #[error("error building http client: `{0}`")]
    HttpClientBuilderError(#[from] HttpClientBuilderError),
}

pub trait OpAMPClientBuilder {
    type Client: StartedClient + 'static;
    fn build_and_start(
        &self,
        opamp_publisher: EventPublisher<OpAMPEvent>,
        agent_id: AgentID,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError>;
}

pub struct DefaultOpAMPClientBuilder<C, B>
where
    B: EffectiveConfigLoaderBuilder,
    C: HttpClientBuilder,
{
    effective_config_loader_builder: B,
    http_client_builder: C,
    poll_interval: Duration,
    disable_startup_check: bool,
}

impl<C, B> DefaultOpAMPClientBuilder<C, B>
where
    B: EffectiveConfigLoaderBuilder,
    C: HttpClientBuilder,
{
    pub fn new(
        http_client_builder: C,
        effective_config_loader_builder: B,
        poll_interval: Duration,
    ) -> Self {
        Self {
            effective_config_loader_builder,
            http_client_builder,
            poll_interval,
            disable_startup_check: false,
        }
    }

    pub fn with_startup_check_disabled(self) -> Self {
        Self {
            disable_startup_check: true,
            ..self
        }
    }
}

impl<C, B> OpAMPClientBuilder for DefaultOpAMPClientBuilder<C, B>
where
    B: EffectiveConfigLoaderBuilder,
    C: HttpClientBuilder,
{
    type Client = StartedHttpClient<
        OpAMPHttpClient<
            AgentCallbacks<<B as EffectiveConfigLoaderBuilder>::Loader>,
            <C as HttpClientBuilder>::Client,
        >,
    >;
    fn build_and_start(
        &self,
        opamp_publisher: EventPublisher<OpAMPEvent>,
        agent_id: AgentID,
        start_settings: StartSettings,
    ) -> Result<Self::Client, OpAMPClientBuilderError> {
        let http_client = self.http_client_builder.build()?;
        let effective_config_loader = self.effective_config_loader_builder.build(agent_id.clone());
        let callbacks = AgentCallbacks::new(agent_id, opamp_publisher, effective_config_loader);
        let not_started_client = NotStartedHttpClient::new(http_client, callbacks, start_settings)?;
        let mut not_started_client = not_started_client.with_interval(self.poll_interval);
        if self.disable_startup_check {
            not_started_client = not_started_client.with_startup_check_disabled();
        }
        info!("OpAMP client started");
        Ok(not_started_client.start()?)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use mockall::{mock, predicate};
    use opamp_client::operation::settings::StartSettings;
    use opamp_client::{
        opamp::proto::{AgentDescription, ComponentHealth, CustomCapabilities, RemoteConfigStatus},
        Client, ClientResult, NotStartedClient, NotStartedClientResult, StartedClient,
        StartedClientResult,
    };

    use super::*;

    mock! {
        pub NotStartedOpAMPClientMock {}
        impl NotStartedClient for NotStartedOpAMPClientMock
        {
            type StartedClient= MockStartedOpAMPClientMock;
            fn start(self) -> NotStartedClientResult<<Self as NotStartedClient>::StartedClient>;
        }
    }

    mock! {
        pub StartedOpAMPClientMock {}

        impl StartedClient for StartedOpAMPClientMock
        {
            fn stop(self) -> StartedClientResult<()>;
        }

        impl Client for StartedOpAMPClientMock
        {
            fn get_agent_description(
                &self,
            ) -> ClientResult<AgentDescription>;

             fn set_agent_description(
                &self,
                description: AgentDescription,
            ) -> ClientResult<()>;

             fn set_health(&self, health: ComponentHealth) -> ClientResult<()>;

             fn update_effective_config(&self) -> ClientResult<()>;

             fn set_remote_config_status(&self, status: RemoteConfigStatus) -> ClientResult<()>;

             fn set_custom_capabilities(&self, capabilities: CustomCapabilities) -> ClientResult<()>;
        }
    }

    impl MockStartedOpAMPClientMock {
        pub fn should_update_effective_config(&mut self, times: usize) {
            self.expect_update_effective_config()
                .times(times)
                .returning(|| Ok(()));
        }
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

        impl OpAMPClientBuilder for OpAMPClientBuilderMock{
            type Client = MockStartedOpAMPClientMock;
            fn build_and_start(&self, opamp_publisher: EventPublisher<OpAMPEvent>, agent_id: AgentID, start_settings: StartSettings) -> Result<<Self as OpAMPClientBuilder>::Client, OpAMPClientBuilderError>;
        }
    }

    impl MockOpAMPClientBuilderMock {
        #[allow(dead_code)] //used in k8s feature
        pub fn should_build_and_start(
            &mut self,
            agent_id: AgentID,
            start_settings: StartSettings,
            client: MockStartedOpAMPClientMock,
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

        // This is a Mock OpAMP Client Builder, which builds the Callbacks and the OpAMP Client
        // and starts the OpAMP Client thread. This thread owns the callbacks, and these publish
        // into the OpAMP Publisher <-- Sub Agent OpAMP Consumer
        // Sub Agent OpAMP Consumer consumes the OpAMP events.
        // Using the Mock, makes the OpAMP publisher to be dropped, as it's part of the expectations
        // Until we refactor these tests (and find a better solution for this pattern) we'll
        // spawn a thread with the publisher, just not to be dropped in the test
        // TL;DR: Let the OpAMP Publisher leave for Duration
        #[cfg(feature = "onhost")]
        pub fn should_build_and_start_and_run(
            &mut self,
            agent_id: AgentID,
            start_settings: StartSettings,
            client: MockStartedOpAMPClientMock,
            run_for: Duration,
        ) {
            use std::thread;
            self.expect_build_and_start()
                .withf(move |publisher, _sub_agent_id, _start_settings| {
                    let publisher = publisher.clone();
                    thread::spawn(move || {
                        thread::sleep(run_for);
                        drop(publisher)
                    });
                    //
                    agent_id == _sub_agent_id.clone() && start_settings == *_start_settings
                })
                .once()
                .return_once(move |_, _, _| Ok(client));
        }
    }
}
