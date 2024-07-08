use super::error::LoaderError;
use crate::opamp::remote_config::ConfigurationMap;
use crate::sub_agent::effective_config::SubAgentEffectiveConfigLoader;
use crate::sub_agent::values::values_repository::ValuesRepository;
use crate::super_agent::config::AgentID;
use std::sync::Arc;

/// Trait for effective configuration loaders.
#[cfg_attr(test, mockall::automock)]
pub trait EffectiveConfigLoader: Send + Sync + 'static {
    /// Load the effective configuration.
    fn load(&self) -> Result<ConfigurationMap, LoaderError>;
}

/// Enumerates all implementations for `EffectiveConfigLoader` for static dispatching reasons.
#[derive(Debug)]
pub enum EffectiveConfigLoaderImpl<R>
where
    R: ValuesRepository,
{
    // TODO this will be replaced with the actual super agent effective config loader.
    SuperAgent(NoOpEffectiveConfigLoader),
    SubAgent(SubAgentEffectiveConfigLoader<R>),

    #[cfg(test)]
    Mock(MockEffectiveConfigLoader),
}

impl<R> EffectiveConfigLoader for EffectiveConfigLoaderImpl<R>
where
    R: ValuesRepository,
{
    fn load(&self) -> Result<ConfigurationMap, LoaderError> {
        match self {
            Self::SuperAgent(loader) => loader.load(),
            Self::SubAgent(loader) => loader.load(),
            #[cfg(test)]
            Self::Mock(mock) => mock.load(),
        }
    }
}

#[cfg_attr(test, mockall::automock)]
/// Builds the `EffectiveConfigLoaderImpl` based on the AgentID.
pub trait EffectiveConfigLoaderBuilder<R: ValuesRepository> {
    fn build(&self, agent_id: AgentID) -> EffectiveConfigLoaderImpl<R>;
}

/// Builder for effective configuration loaders.
pub struct DefaultEffectiveConfigLoaderBuilder<R> {
    sub_agent_values_repository: Arc<R>,
}

impl<R> DefaultEffectiveConfigLoaderBuilder<R> {
    pub fn new(sub_agent_values_repository: Arc<R>) -> Self {
        Self {
            sub_agent_values_repository,
        }
    }
}

impl<R> EffectiveConfigLoaderBuilder<R> for DefaultEffectiveConfigLoaderBuilder<R>
where
    R: ValuesRepository,
{
    fn build(&self, agent_id: AgentID) -> EffectiveConfigLoaderImpl<R> {
        if agent_id.is_super_agent_id() {
            return EffectiveConfigLoaderImpl::SuperAgent(NoOpEffectiveConfigLoader);
        }

        let loader =
            SubAgentEffectiveConfigLoader::new(agent_id, self.sub_agent_values_repository.clone());

        EffectiveConfigLoaderImpl::SubAgent(loader)
    }
}

/// A no-op effective configuration loader that always returns an empty configuration.
#[derive(Debug)]
pub struct NoOpEffectiveConfigLoader;

/// Implementation of the `EffectiveConfigLoader` trait for the no-op loader. Returns an empty configuration.
impl EffectiveConfigLoader for NoOpEffectiveConfigLoader {
    fn load(&self) -> Result<ConfigurationMap, LoaderError> {
        Ok(ConfigurationMap::default())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::sub_agent::values::values_repository::test::MockRemoteValuesRepositoryMock;

    #[test]
    fn builder() {
        let builder = DefaultEffectiveConfigLoaderBuilder::new(Arc::new(
            MockRemoteValuesRepositoryMock::default(),
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
            sub_agent_values_repository: Arc::new(MockRemoteValuesRepositoryMock::default()),
        };

        let loader = loader_builder.build(AgentID::new_super_agent_id());
        let config = loader.load().unwrap();
        assert_eq!(config, ConfigurationMap::default());
    }
}
