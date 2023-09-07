use opamp_client::{httpclient::HttpClient, operation::settings::StartSettings, OpAMPClient};

use crate::config::agent_configs::OpAMPClientConfig;

use super::{
    callbacks::{AgentCallbacks, AgentEffectiveConfig},
    error::AgentError,
};

pub trait OpAMPClientBuilder {
    type Client: OpAMPClient;
    fn build(&self, start_settings: StartSettings) -> Result<Self::Client, AgentError>;
}

/// OpAMPBuilderCfg
pub struct OpAMPHttpBuilder {
    config: OpAMPClientConfig,
}

impl OpAMPHttpBuilder {
    pub(crate) fn new(config: OpAMPClientConfig) -> Self {
        Self { config }
    }
}

impl OpAMPClientBuilder for OpAMPHttpBuilder {
    type Client = HttpClient<AgentEffectiveConfig, AgentCallbacks>;
    fn build(
        &self,
        start_settings: StartSettings,
    ) -> Result<Self::Client, super::error::AgentError> {
        // TODO: cleanup
        let headers = self.config.headers.clone().unwrap_or_default();
        let headers: Vec<(&str, &str)> = headers
            .iter()
            .map(|header| (header.0.as_str(), header.1.as_str()))
            .collect();

        Ok(HttpClient::new(
            AgentEffectiveConfig,
            self.config.endpoint.as_str(),
            headers,
            start_settings,
            AgentCallbacks,
        )
        .unwrap())
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use async_trait::async_trait;
    use mockall::mock;
    use opamp_client::{
        opamp::proto::{AgentDescription, AgentHealth},
        OpAMPClient, OpAMPClientHandle,
    };

    use crate::agent::error::AgentError;

    mock! {
        pub OpAMPClientMock {}

        #[async_trait]
        impl OpAMPClient for OpAMPClientMock {
            type Handle = MockOpAMPClientMock;
            type Error = AgentError;
            // add code here
            async fn start(self) -> Result<<Self as OpAMPClient>::Handle, <Self as OpAMPClient>::Error>;
        }

        #[async_trait]
        impl OpAMPClientHandle for OpAMPClientMock {
            type Error = AgentError;

            async fn stop(self) -> Result<(), <Self as OpAMPClientHandle>::Error>;

            async fn set_agent_description(
                &mut self,
                description: &AgentDescription,
            ) -> Result<(), <Self as OpAMPClientHandle>::Error>;

            fn agent_description(&self) -> Result<AgentDescription, <Self as OpAMPClientHandle>::Error>;

            async fn set_health(&mut self, health: &AgentHealth) -> Result<(), <Self as OpAMPClientHandle>::Error>;

            async fn update_effective_config(&mut self) -> Result<(), <Self as OpAMPClientHandle>::Error>;
        }
    }

    mock! {
        pub OpAMPClientBuilderMock {}

        impl OpAMPClientBuilder for OpAMPClientBuilderMock {
            type Client = MockOpAMPClientMock;

            fn build(&self, start_settings: opamp_client::operation::settings::StartSettings) -> Result<<Self as OpAMPClientBuilder>::Client, AgentError>;
        }
    }
}
