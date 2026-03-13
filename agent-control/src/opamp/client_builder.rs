use super::callbacks::AgentCallbacks;
use super::effective_config::loader::EffectiveConfigLoaderBuilder;
use super::http::builder::{HttpClientBuilder, HttpClientBuilderError};
use super::instance_id::getter::GetterError;
use crate::event::OpAMPEvent;
use crate::event::channel::{EventConsumer, pub_sub};
use crate::opamp::instance_id::InstanceID;
use crate::opamp::operations::start_settings;
use crate::sub_agent::identity::AgentIdentity;
use duration_str::deserialize_duration;
use opamp_client::http::client::OpAMPHttpClient;
use opamp_client::http::{NotStartedHttpClient, StartedHttpClient};
use opamp_client::operation::settings::DescriptionValueType;
use opamp_client::{NotStartedClient, NotStartedClientError, StartedClient};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::info;
use wrapper_with_default::WrapperWithDefault;

/// Default poll interval for the OpAMP http managed client
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, WrapperWithDefault)]
#[wrapper_default_value(DEFAULT_POLL_INTERVAL)]
pub struct PollInterval(#[serde(deserialize_with = "deserialize_duration")] Duration);

#[derive(Error, Debug)]
pub enum OpAMPClientBuilderError {
    #[error("OpAMP client: {0}")]
    NotStartedClientError(#[from] NotStartedClientError),

    #[error("error getting agent instance id: {0}")]
    GetInstanceIDError(#[from] GetterError),

    #[error("error building http client: {0}")]
    HttpClientBuilderError(#[from] HttpClientBuilderError),
}

pub trait OpAMPClientBuilder: Clone {
    type Client: StartedClient + 'static;

    fn with_agent_identity(self, agent_identity: AgentIdentity) -> Self;

    fn with_additional_identifying_attributes(
        self,
        additional_identifying_attributes: HashMap<String, DescriptionValueType>,
    ) -> Self;

    fn with_non_identifying_attributes(
        self,
        non_identifying_attributes: HashMap<String, DescriptionValueType>,
    ) -> Self;

    fn build_and_start(
        &self,
    ) -> Result<(Self::Client, EventConsumer<OpAMPEvent>), OpAMPClientBuilderError>;
}

pub struct OpAMPClientBuilderImpl<C, B>
where
    B: EffectiveConfigLoaderBuilder,
    C: HttpClientBuilder,
{
    effective_config_loader_builder: Arc<B>,
    http_client_builder: Arc<C>,
    poll_interval: PollInterval,
    disable_startup_check: bool,
    instance_id: InstanceID,

    agent_identity: AgentIdentity,
    additional_identifying_attributes: HashMap<String, DescriptionValueType>,
    non_identifying_attributes: HashMap<String, DescriptionValueType>,
}

impl<C, B> Clone for OpAMPClientBuilderImpl<C, B>
where
    B: EffectiveConfigLoaderBuilder,
    C: HttpClientBuilder,
{
    fn clone(&self) -> Self {
        Self {
            effective_config_loader_builder: self.effective_config_loader_builder.clone(),
            http_client_builder: self.http_client_builder.clone(),
            poll_interval: self.poll_interval,
            disable_startup_check: self.disable_startup_check,
            instance_id: self.instance_id.clone(),
            agent_identity: self.agent_identity.clone(),
            additional_identifying_attributes: self.additional_identifying_attributes.clone(),
            non_identifying_attributes: self.non_identifying_attributes.clone(),
        }
    }
}

type NotStartedOpAMPClient<B, C> = NotStartedHttpClient<
    OpAMPHttpClient<
        AgentCallbacks<<B as EffectiveConfigLoaderBuilder>::Loader>,
        <C as HttpClientBuilder>::Client,
    >,
>;

impl<C, B> OpAMPClientBuilderImpl<C, B>
where
    B: EffectiveConfigLoaderBuilder,
    C: HttpClientBuilder,
{
    pub fn new(
        poll_interval: PollInterval,
        http_client_builder: Arc<C>,
        effective_config_loader_builder: Arc<B>,
        instance_id: InstanceID,
    ) -> Self {
        Self {
            effective_config_loader_builder,
            http_client_builder,
            poll_interval,
            disable_startup_check: false,
            instance_id,
            agent_identity: AgentIdentity::new_agent_control_identity(),
            additional_identifying_attributes: HashMap::new(),
            non_identifying_attributes: HashMap::new(),
        }
    }

    pub fn with_startup_check_disabled(mut self) -> Self {
        self.disable_startup_check = true;
        self
    }

    pub fn with_agent_identity(mut self, agent_identity: AgentIdentity) -> Self {
        self.agent_identity = agent_identity;
        self
    }

    pub fn build(
        &self,
    ) -> Result<(NotStartedOpAMPClient<B, C>, EventConsumer<OpAMPEvent>), OpAMPClientBuilderError>
    {
        let (publisher, consumer) = pub_sub::<OpAMPEvent>();
        let start_settings = start_settings(
            self.instance_id.clone(),
            &self.agent_identity,
            self.additional_identifying_attributes.clone(),
            self.non_identifying_attributes.clone(),
        );

        let http_client = self.http_client_builder.build()?;
        let effective_config_loader = self
            .effective_config_loader_builder
            .build(self.agent_identity.id.clone());

        let callbacks = AgentCallbacks::new(
            self.agent_identity.id.clone(),
            publisher,
            effective_config_loader,
        );
        let not_started_client = NotStartedHttpClient::new(http_client, callbacks, start_settings)?;
        let mut not_started_client = not_started_client.with_interval(self.poll_interval.into());
        if self.disable_startup_check {
            not_started_client = not_started_client.with_startup_check_disabled();
        }

        Ok((not_started_client, consumer))
    }
}

impl<C, B> OpAMPClientBuilder for OpAMPClientBuilderImpl<C, B>
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

    fn with_agent_identity(mut self, agent_identity: AgentIdentity) -> Self {
        self.agent_identity = agent_identity;
        self
    }

    fn with_additional_identifying_attributes(
        mut self,
        additional_identifying_attributes: HashMap<String, DescriptionValueType>,
    ) -> Self {
        self.additional_identifying_attributes = additional_identifying_attributes;
        self
    }

    fn with_non_identifying_attributes(
        mut self,
        non_identifying_attributes: HashMap<String, DescriptionValueType>,
    ) -> Self {
        self.non_identifying_attributes = non_identifying_attributes;
        self
    }

    fn build_and_start(
        &self,
    ) -> Result<(Self::Client, EventConsumer<OpAMPEvent>), OpAMPClientBuilderError> {
        let disable_startup_check = self.disable_startup_check;
        let poll_interval = self.poll_interval;

        let (not_started_client, consumer) = self.build()?;
        let mut not_started_client = not_started_client.with_interval(poll_interval.into());
        if disable_startup_check {
            not_started_client = not_started_client.with_startup_check_disabled();
        }

        info!("OpAMP client started");
        Ok((not_started_client.start()?, consumer))
    }
}

#[cfg(test)]
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

        impl OpAMPClientBuilder for OpAMPClientBuilder{
            type Client = MockStartedOpAMPClient;

            fn with_agent_identity(self, agent_identity: AgentIdentity) -> Self;

            fn with_additional_identifying_attributes(
                self,
                additional_identifying_attributes: HashMap<String, DescriptionValueType>,
            ) -> Self;

            fn with_non_identifying_attributes(
                self,
                non_identifying_attributes: HashMap<String, DescriptionValueType>,
            ) -> Self;

            fn build_and_start(&self) -> Result<(<Self as OpAMPClientBuilder>::Client, EventConsumer<OpAMPEvent>), OpAMPClientBuilderError>;
        }

        impl Clone for OpAMPClientBuilder {
            fn clone(&self) -> Self;
        }
    }

    impl MockOpAMPClientBuilder {
        pub fn should_build_and_start(&mut self, client: MockStartedOpAMPClient) {
            let (_publisher, consumer) = pub_sub::<OpAMPEvent>();
            self.expect_build_and_start()
                .once()
                .return_once(move || Ok((client, consumer)));
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
            client: MockStartedOpAMPClient,
            run_for: Duration,
        ) {
            use std::thread;
            let (_publisher, consumer) = pub_sub::<OpAMPEvent>();
            self.expect_build_and_start()
                .withf(move || {
                    thread::spawn(move || {
                        thread::sleep(run_for);
                    });
                    true
                })
                .once()
                .return_once(move || Ok((client, consumer)));
        }
    }
}
