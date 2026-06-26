//! Builds and starts the OpAMP HTTP client, wiring callbacks, effective-config loader and HTTP client.
use super::callbacks::AgentCallbacks;
use super::effective_config::loader::BuildEffectiveConfigLoader;
use super::http::builder::{HttpClientBuilder, HttpClientBuilderError};
use super::instance_id::getter::GetterError;
use crate::event::OpAMPEvent;
use crate::event::channel::{EventConsumer, pub_sub};
use crate::sub_agent::identity::AgentIdentity;
use duration_str::deserialize_duration;
use opamp_client::http::client::OpAMPHttpClient;
use opamp_client::http::{NotStartedHttpClient, StartedHttpClient};
use opamp_client::operation::settings::StartSettings;
use opamp_client::{NotStartedClient, NotStartedClientError, StartedClient};
use serde::Deserialize;
use std::time::Duration;
use thiserror::Error;
use tracing::info;
use wrapper_with_default::WrapperWithDefault;

/// Default poll interval for the OpAMP http managed client
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);
/// Interval between OpAMP poll requests, defaulting to [`DEFAULT_POLL_INTERVAL`].
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_POLL_INTERVAL)]
pub struct PollInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

/// Errors that can occur while building or starting the OpAMP client.
#[derive(Error, Debug)]
pub enum OpAMPClientBuilderError {
    /// The OpAMP client could not be created.
    #[error("OpAMP client: {0}")]
    NotStartedClientError(#[from] NotStartedClientError),

    /// The agent's instance id could not be obtained.
    #[error("error getting agent instance id: {0}")]
    GetInstanceIDError(#[from] GetterError),

    /// The underlying HTTP client could not be built.
    #[error("error building http client: {0}")]
    HttpClientBuilderError(#[from] HttpClientBuilderError),
}

/// Builds and starts an OpAMP client for a given agent identity and start settings.
pub trait BuildOpAMPClient {
    /// The started OpAMP client type produced by this builder.
    type Client: StartedClient + 'static;

    /// Builds the OpAMP client, starts it, and returns it along with its event consumer.
    fn build_and_start(
        &self,
        agent_identity: AgentIdentity,
        start_settings: StartSettings,
    ) -> Result<(Self::Client, EventConsumer<OpAMPEvent>), OpAMPClientBuilderError>;
}

type NotStartedOpAMPClient<B, C> = NotStartedHttpClient<
    OpAMPHttpClient<
        AgentCallbacks<<B as BuildEffectiveConfigLoader>::Loader>,
        <C as HttpClientBuilder>::Client,
    >,
>;

/// Default [`BuildOpAMPClient`] implementation backed by an HTTP client builder and an
/// effective-config loader builder.
pub struct OpAMPClientBuilder<C, B>
where
    B: BuildEffectiveConfigLoader,
    C: HttpClientBuilder,
{
    effective_config_loader_builder: B,
    http_client_builder: C,
    poll_interval: PollInterval,
    disable_startup_check: bool,
}

impl<C, B> OpAMPClientBuilder<C, B>
where
    B: BuildEffectiveConfigLoader,
    C: HttpClientBuilder,
{
    /// Creates a builder with the given poll interval, HTTP client builder and effective-config
    /// loader builder.
    pub fn new(
        poll_interval: PollInterval,
        http_client_builder: C,
        effective_config_loader_builder: B,
    ) -> Self {
        Self {
            effective_config_loader_builder,
            http_client_builder,
            poll_interval,
            disable_startup_check: false,
        }
    }

    /// Returns the builder with the OpAMP client's startup check disabled.
    pub fn with_startup_check_disabled(self) -> Self {
        Self {
            disable_startup_check: true,
            ..self
        }
    }

    /// Builds the (not yet started) OpAMP client and returns it with its event consumer.
    pub fn build(
        &self,
        agent_identity: AgentIdentity,
        start_settings: StartSettings,
    ) -> Result<(NotStartedOpAMPClient<B, C>, EventConsumer<OpAMPEvent>), OpAMPClientBuilderError>
    {
        let (publisher, consumer) = pub_sub::<OpAMPEvent>();

        let http_client = self.http_client_builder.build()?;
        let effective_config_loader = self
            .effective_config_loader_builder
            .build(agent_identity.id.clone());

        let callbacks = AgentCallbacks::new(agent_identity.id, publisher, effective_config_loader);
        let not_started_client = NotStartedHttpClient::new(http_client, callbacks, start_settings)?;
        let mut not_started_client = not_started_client.with_interval(self.poll_interval.into());
        if self.disable_startup_check {
            not_started_client = not_started_client.with_startup_check_disabled();
        }

        Ok((not_started_client, consumer))
    }
}

impl<C, B> BuildOpAMPClient for OpAMPClientBuilder<C, B>
where
    B: BuildEffectiveConfigLoader,
    C: HttpClientBuilder,
{
    type Client = StartedHttpClient<OpAMPHttpClient<AgentCallbacks<<B>::Loader>, <C>::Client>>;

    fn build_and_start(
        &self,
        agent_identity: AgentIdentity,
        start_settings: StartSettings,
    ) -> Result<(Self::Client, EventConsumer<OpAMPEvent>), OpAMPClientBuilderError> {
        let (not_started_client, consumer) = self.build(agent_identity, start_settings)?;

        info!("OpAMP client started");
        Ok((not_started_client.start()?, consumer))
    }
}

#[cfg(test)]
#[allow(missing_docs)] // test-support code
pub(crate) mod tests {
    use mockall::{Sequence, mock, predicate};
    use opamp_client::{
        Client, ClientResult, NotStartedClient, NotStartedClientResult, StartedClient,
        StartedClientResult,
        opamp::proto::{AgentDescription, ComponentHealth, CustomCapabilities, RemoteConfigStatus},
    };

    use super::*;

    mock! {
        pub NotStartedOpAMPClient {}
        impl NotStartedClient for NotStartedOpAMPClient
        {
            type StartedClient= MockStartedOpAMPClient;
            fn start(self) -> NotStartedClientResult<<Self as NotStartedClient>::StartedClient>;
        }
    }

    mock! {
        pub StartedOpAMPClient {}

        impl StartedClient for StartedOpAMPClient
        {
            fn stop(self) -> StartedClientResult<()>;
        }

        impl Client for StartedOpAMPClient
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

    impl MockStartedOpAMPClient {
        pub fn should_update_effective_config(&mut self, times: usize) {
            self.expect_update_effective_config()
                .times(times)
                .returning(|| Ok(()));
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

        pub fn should_set_remote_config_status(&mut self, status: RemoteConfigStatus) {
            self.expect_set_remote_config_status()
                .once()
                .with(predicate::eq(status))
                .returning(|_| Ok(()));
        }

        pub fn should_set_remote_config_status_seq(&mut self, status_seq: Vec<RemoteConfigStatus>) {
            let mut sequence = Sequence::new();
            for status in status_seq {
                self.expect_set_remote_config_status()
                    .once()
                    .in_sequence(&mut sequence)
                    .with(predicate::eq(status))
                    .returning(|_| Ok(()));
            }
        }

        /// Same as [Self::should_set_remote_config_status_seq] but it ignores the `last_error` field
        pub fn should_set_remote_config_status_matching_seq(
            &mut self,
            status_seq: Vec<RemoteConfigStatus>,
        ) {
            let mut sequence = Sequence::new();
            for status in status_seq {
                self.expect_set_remote_config_status()
                    .once()
                    .in_sequence(&mut sequence)
                    .with(predicate::function(move |arg: &RemoteConfigStatus| {
                        status.status == arg.status
                            && status.last_remote_config_hash == arg.last_remote_config_hash
                    }))
                    .returning(|_| Ok(()));
            }
        }
    }

    mock! {
        pub OpAMPClientBuilder {}

        impl BuildOpAMPClient for OpAMPClientBuilder{
            type Client = MockStartedOpAMPClient;

            fn build_and_start(
                &self,
                agent_identity: AgentIdentity,
                start_settings: StartSettings,
            ) -> Result<(<Self as BuildOpAMPClient>::Client, EventConsumer<OpAMPEvent>), OpAMPClientBuilderError>;
        }
    }

    impl MockOpAMPClientBuilder {
        pub fn should_build_and_start(
            &mut self,
            agent_identity: AgentIdentity,
            start_settings: StartSettings,
            client: MockStartedOpAMPClient,
        ) {
            let (_publisher, consumer) = pub_sub::<OpAMPEvent>();
            self.expect_build_and_start()
                .with(predicate::eq(agent_identity), predicate::eq(start_settings))
                .once()
                .return_once(move |_, _| Ok((client, consumer)));
        }

        // This is a Mock OpAMP Client Builder, which builds the Callbacks and the OpAMP Client
        // and starts the OpAMP Client thread. This thread owns the callbacks, and these publish
        // into the OpAMP Publisher <-- Sub Agent OpAMP Consumer
        // Sub Agent OpAMP Consumer consumes the OpAMP events.
        // Using the Mock, makes the OpAMP publisher to be dropped, as it's part of the expectations
        // Until we refactor these tests (and find a better solution for this pattern) we'll
        // spawn a thread with the publisher, just not to be dropped in the test
        // TL;DR: Let the OpAMP Publisher leave for Duration

        pub fn should_build_and_start_and_run(
            &mut self,
            expected_agent_identity: AgentIdentity,
            expected_start_settings: StartSettings,
            client: MockStartedOpAMPClient,
            run_for: Duration,
        ) {
            use std::thread;
            let (_publisher, consumer) = pub_sub::<OpAMPEvent>();
            self.expect_build_and_start()
                .withf(move |agent_identity, start_settings| {
                    thread::spawn(move || {
                        thread::sleep(run_for);
                    });
                    *agent_identity == expected_agent_identity
                        && *start_settings == expected_start_settings
                })
                .return_once(move |_, _| Ok((client, consumer)));
        }
    }
}
