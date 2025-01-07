use super::agent_control::AgentControlEffectiveConfigLoader;
use super::error::LoaderError;
use super::sub_agent::SubAgentEffectiveConfigLoader;
use crate::agent_control::config::AgentID;
use crate::opamp::remote_config::status_manager::ConfigStatusManager;
use crate::opamp::remote_config::ConfigurationMap;
use std::sync::Arc;

/// Trait for effective configuration loaders.
/// IMPORTANT NOTE: Effective config must be restricted to:
/// - Contain only values that can be modified through opamp remote configs.
/// - Doesn’t contain the real values but the same config defined by users.
///   Meaning no default values should be present.
/// - Doesn’t contain configs that have been set by environment variables.
/// - If a config has an environment variable placeholder, it should be reported as it is.
///   It should never contain the resolved value.
pub trait EffectiveConfigLoader: Send + Sync + 'static {
    /// Load the effective configuration.
    fn load(&self) -> Result<ConfigurationMap, LoaderError>;
}

pub trait EffectiveConfigLoaderBuilder {
    type Loader: EffectiveConfigLoader;

    fn build(&self, agent_id: AgentID) -> Self::Loader;
}

/// Builder for effective configuration loaders.
pub struct DefaultEffectiveConfigLoaderBuilder<M>
where
    M: ConfigStatusManager,
{
    config_manager: Arc<M>,
}

impl<M> DefaultEffectiveConfigLoaderBuilder<M>
where
    M: ConfigStatusManager,
{
    pub fn new(config_manager: Arc<M>) -> Self {
        Self { config_manager }
    }
}

impl<M> EffectiveConfigLoaderBuilder for DefaultEffectiveConfigLoaderBuilder<M>
where
    M: ConfigStatusManager + Send + Sync + 'static,
{
    type Loader = EffectiveConfigLoaderImpl<M>;

    fn build(&self, agent_id: AgentID) -> Self::Loader {
        if agent_id.is_agent_control_id() {
            return EffectiveConfigLoaderImpl::AgentControl(
                AgentControlEffectiveConfigLoader::new(self.config_manager.clone()),
            );
        }
        EffectiveConfigLoaderImpl::SubAgent(SubAgentEffectiveConfigLoader::new(
            agent_id,
            self.config_manager.clone(),
        ))
    }
}

/// Enumerates all implementations for `EffectiveConfigLoader` for static dispatching reasons.
pub enum EffectiveConfigLoaderImpl<M>
where
    M: ConfigStatusManager,
{
    AgentControl(AgentControlEffectiveConfigLoader<M>),
    SubAgent(SubAgentEffectiveConfigLoader<M>),
}

impl<M> EffectiveConfigLoader for EffectiveConfigLoaderImpl<M>
where
    M: ConfigStatusManager + Send + Sync + 'static,
{
    fn load(&self) -> Result<ConfigurationMap, LoaderError> {
        match self {
            Self::AgentControl(loader) => loader.load(),
            Self::SubAgent(loader) => loader.load(),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use mockall::mock;

    use crate::opamp::remote_config::status_manager::tests::MockConfigStatusManagerMock;

    use super::*;

    mock!(
        pub EffectiveConfigLoaderMock {}

        impl EffectiveConfigLoader for EffectiveConfigLoaderMock {
            fn load(&self) -> Result<ConfigurationMap, LoaderError>;
        }
    );

    mock! {
        pub EffectiveConfigLoaderBuilderMock {}

        impl EffectiveConfigLoaderBuilder for EffectiveConfigLoaderBuilderMock {
            type Loader = MockEffectiveConfigLoaderMock;

            fn build(&self,agent_id: AgentID) -> MockEffectiveConfigLoaderMock;
        }
    }
    #[test]
    fn builder() {
        let builder =
            DefaultEffectiveConfigLoaderBuilder::new(Arc::new(MockConfigStatusManagerMock::new()));

        match builder.build(AgentID::new_agent_control_id()) {
            EffectiveConfigLoaderImpl::AgentControl(_) => {}
            _ => panic!("Expected AgentControl loader"),
        }

        match builder.build(AgentID::new("test").unwrap()) {
            EffectiveConfigLoaderImpl::SubAgent(_) => {}
            _ => panic!("Expected SubAgent loader"),
        }
    }
}
