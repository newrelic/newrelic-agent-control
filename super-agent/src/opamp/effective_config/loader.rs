use super::error::LoaderError;
use super::sub_agent::SubAgentEffectiveConfigLoader;
use super::super_agent::SuperAgentEffectiveConfigLoader;
use crate::opamp::remote_config::ConfigurationMap;
use crate::super_agent::config::AgentID;
use crate::values::yaml_config_repository::YAMLConfigRepository;
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
pub struct DefaultEffectiveConfigLoaderBuilder<Y>
where
    Y: YAMLConfigRepository,
{
    sub_agent_repository: Arc<Y>,
    super_agent_repository: Arc<Y>,
}

impl<Y> DefaultEffectiveConfigLoaderBuilder<Y>
where
    Y: YAMLConfigRepository,
{
    pub fn new(sub_agent_repository: Arc<Y>, super_agent_repository: Arc<Y>) -> Self {
        Self {
            sub_agent_repository,
            super_agent_repository,
        }
    }
}

impl<Y> EffectiveConfigLoaderBuilder for DefaultEffectiveConfigLoaderBuilder<Y>
where
    Y: YAMLConfigRepository,
{
    type Loader = EffectiveConfigLoaderImpl<Y>;

    fn build(&self, agent_id: AgentID) -> Self::Loader {
        if agent_id.is_super_agent_id() {
            return EffectiveConfigLoaderImpl::SuperAgent(SuperAgentEffectiveConfigLoader::new(
                self.super_agent_repository.clone(),
            ));
        }
        EffectiveConfigLoaderImpl::SubAgent(SubAgentEffectiveConfigLoader::new(
            agent_id,
            self.sub_agent_repository.clone(),
        ))
    }
}

/// Enumerates all implementations for `EffectiveConfigLoader` for static dispatching reasons.
pub enum EffectiveConfigLoaderImpl<Y>
where
    Y: YAMLConfigRepository,
{
    SuperAgent(SuperAgentEffectiveConfigLoader<Y>),
    SubAgent(SubAgentEffectiveConfigLoader<Y>),
}

impl<Y> EffectiveConfigLoader for EffectiveConfigLoaderImpl<Y>
where
    Y: YAMLConfigRepository,
{
    fn load(&self) -> Result<ConfigurationMap, LoaderError> {
        match self {
            Self::SuperAgent(loader) => loader.load(),
            Self::SubAgent(loader) => loader.load(),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use mockall::mock;

    use crate::values::yaml_config_repository::test::MockYAMLConfigRepositoryMock;

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
        let builder = DefaultEffectiveConfigLoaderBuilder::new(
            Arc::new(MockYAMLConfigRepositoryMock::default()),
            Arc::new(MockYAMLConfigRepositoryMock::default()),
        );

        match builder.build(AgentID::new_super_agent_id()) {
            EffectiveConfigLoaderImpl::SuperAgent(_) => {}
            _ => panic!("Expected SuperAgent loader"),
        }

        match builder.build(AgentID::new("test").unwrap()) {
            EffectiveConfigLoaderImpl::SubAgent(_) => {}
            _ => panic!("Expected SubAgent loader"),
        }
    }
}
