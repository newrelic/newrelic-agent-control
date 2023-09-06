use opamp_client::{
    capabilities, httpclient::HttpClient, opamp::proto::AgentCapabilities,
    operation::settings::StartSettings, OpAMPClient,
};

use super::{
    callbacks::{AgentCallbacks, AgentEffectiveConfig},
    error::AgentError,
};

pub trait OpAMPClientBuilder {
    type Client: OpAMPClient;
    fn build(&self) -> Result<Self::Client, AgentError>;
}

/// OpAMPBuilderCfg
pub struct OpAMPHttpBuilder;

impl OpAMPHttpBuilder {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl OpAMPClientBuilder for OpAMPHttpBuilder {
    type Client = HttpClient<AgentEffectiveConfig, AgentCallbacks>;
    fn build(&self) -> Result<Self::Client, super::error::AgentError> {
        Ok(HttpClient::new(
            AgentEffectiveConfig,
            "test",
            [("hi", "bye")],
            StartSettings {
                instance_id: "jfkdlsa".to_string(),
                capabilities: capabilities!(AgentCapabilities::ReportsStatus),
            },
            AgentCallbacks,
        )
        .unwrap())
    }
}
