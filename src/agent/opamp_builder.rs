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
    pub struct OpAMPClientMock;
    pub struct OpAMPClientBuilderMock;

    use async_trait::async_trait;
    use opamp_client::{
        opamp::proto::{AgentDescription, AgentHealth},
        OpAMPClient, OpAMPClientHandle,
    };

    use crate::agent::error::AgentError;

    use super::OpAMPClientBuilder;

    #[async_trait]
    impl OpAMPClient for OpAMPClientMock {
        type Handle = OpAMPClientMock;
        type Error = AgentError;
        async fn start(self) -> Result<Self::Handle, Self::Error> {
            Ok(OpAMPClientMock)
        }
    }
    #[async_trait]
    impl OpAMPClientHandle for OpAMPClientMock {
        type Error = AgentError;

        async fn stop(self) -> Result<(), Self::Error> {
            Ok(())
        }

        /// set_agent_description sets attributes of the Agent. The attributes will be included
        /// in the next status report sent to the Server.
        async fn set_agent_description(
            &mut self,
            _description: &AgentDescription,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        /// agent_description returns the last value successfully set by set_agent_description().
        fn agent_description(&self) -> Result<AgentDescription, Self::Error> {
            Ok(AgentDescription::default())
        }

        /// set_health sets the health status of the Agent. The AgentHealth will be included
        async fn set_health(&mut self, _health: &AgentHealth) -> Result<(), Self::Error> {
            Ok(())
        }

        // update_effective_config fetches the current local effective config using
        // get_effective_config callback and sends it to the Server.
        async fn update_effective_config(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    impl OpAMPClientBuilder for OpAMPClientBuilderMock {
        type Client = OpAMPClientMock;
        fn build(
            &self,
            _start_settings: opamp_client::operation::settings::StartSettings,
        ) -> Result<Self::Client, AgentError> {
            Ok(OpAMPClientMock)
        }
    }
}
