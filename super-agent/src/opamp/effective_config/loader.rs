use super::error::LoaderError;
use super::sub_agent::SubAgentEffectiveConfigLoader;
use crate::opamp::remote_config::ConfigurationMap;
use crate::super_agent::config::AgentID;
use crate::values::yaml_config_repository::YAMLConfigRepository;
use std::sync::Arc;

/// Trait for effective configuration loaders.
pub trait EffectiveConfigLoader: Send + Sync + 'static {
    /// Load the effective configuration.
    fn load(&self) -> Result<ConfigurationMap, LoaderError>;
}

pub trait EffectiveConfigLoaderBuilder {
    type Loader: EffectiveConfigLoader;

    fn build(&self, agent_id: AgentID) -> Self::Loader;
}

/// Builder for effective configuration loaders.
pub struct DefaultEffectiveConfigLoaderBuilder<R>
where
    R: YAMLConfigRepository,
{
    yaml_config_repository: Arc<R>,
}

impl<R> DefaultEffectiveConfigLoaderBuilder<R>
where
    R: YAMLConfigRepository,
{
    pub fn new(yaml_config_repository: Arc<R>) -> Self {
        Self {
            yaml_config_repository,
        }
    }
}

impl<R> EffectiveConfigLoaderBuilder for DefaultEffectiveConfigLoaderBuilder<R>
where
    R: YAMLConfigRepository,
{
    type Loader = EffectiveConfigLoaderImpl<R>;

    fn build(&self, agent_id: AgentID) -> Self::Loader {
        if agent_id.is_super_agent_id() {
            return EffectiveConfigLoaderImpl::SuperAgent(NoOpEffectiveConfigLoader);
        }

        let loader =
            SubAgentEffectiveConfigLoader::new(agent_id, self.yaml_config_repository.clone());

        EffectiveConfigLoaderImpl::SubAgent(loader)
    }
}

/// Enumerates all implementations for `EffectiveConfigLoader` for static dispatching reasons.
pub enum EffectiveConfigLoaderImpl<R>
where
    R: YAMLConfigRepository,
{
    // TODO this will be replaced with the actual super agent effective config loader.
    SuperAgent(NoOpEffectiveConfigLoader),
    SubAgent(SubAgentEffectiveConfigLoader<R>),
}

impl<R> EffectiveConfigLoader for EffectiveConfigLoaderImpl<R>
where
    R: YAMLConfigRepository,
{
    fn load(&self) -> Result<ConfigurationMap, LoaderError> {
        match self {
            Self::SuperAgent(loader) => loader.load(),
            Self::SubAgent(loader) => loader.load(),
        }
    }
}

/// A no-op effective configuration loader that always returns an empty configuration.
pub struct NoOpEffectiveConfigLoader;

/// Implementation of the `EffectiveConfigLoader` trait for the no-op loader. Returns an empty configuration.
impl EffectiveConfigLoader for NoOpEffectiveConfigLoader {
    fn load(&self) -> Result<ConfigurationMap, LoaderError> {
        Ok(ConfigurationMap::default())
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
        let builder = DefaultEffectiveConfigLoaderBuilder::new(Arc::new(
            MockYAMLConfigRepositoryMock::default(),
        ));

        match builder.build(AgentID::new_super_agent_id()) {
            EffectiveConfigLoaderImpl::SuperAgent(_) => {}
            _ => panic!("Expected SuperAgent loader"),
        }

        match builder.build(AgentID::new("test").unwrap()) {
            EffectiveConfigLoaderImpl::SubAgent(_) => {}
            _ => panic!("Expected SubAgent loader"),
        }
    }

    #[test]
    fn no_op_loader() {
        let loader_builder = DefaultEffectiveConfigLoaderBuilder {
            yaml_config_repository: Arc::new(MockYAMLConfigRepositoryMock::default()),
        };

        let loader = loader_builder.build(AgentID::new_super_agent_id());
        let config = loader.load().unwrap();
        assert_eq!(config, ConfigurationMap::default());
    }
}
