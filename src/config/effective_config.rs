use opamp_client::opamp::proto::EffectiveConfig;
use opamp_client::operation::agent::Agent;
use thiserror::Error;

use crate::config::error::SuperAgentConfigError;

pub struct EffectiveConfigRetriever {
    path: String,
}

impl EffectiveConfigRetriever {
    pub fn new(path: String) -> Self {
        Self { path }
    }
}

#[derive(Error, Debug)]
pub enum EffectiveConfigRetrieverError {
    #[error("cannot retrieve effective config")]
    EffectiveConfigRetrieveError(#[from] SuperAgentConfigError),
}

impl Agent for EffectiveConfigRetriever {
    type Error = EffectiveConfigRetrieverError;

    fn get_effective_config(&self) -> Result<EffectiveConfig, Self::Error> {
        // let path = Path::new(self.path.as_str());
        // let super_agent_config = Resolver::retrieve_config(&path)?;
        todo!()
    }
}
