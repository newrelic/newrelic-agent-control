use super::agent_control::AgentControlEffectiveConfigLoader;
use super::error::LoaderError;
use super::sub_agent::SubAgentEffectiveConfigLoader;
use crate::agent_control::agent_id::AgentID;
use crate::opamp::remote_config::ConfigurationMap;
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
    yaml_config_repository: Arc<Y>,
}

impl<Y> DefaultEffectiveConfigLoaderBuilder<Y>
where
    Y: YAMLConfigRepository,
{
    pub fn new(yaml_config_repository: Arc<Y>) -> Self {
        Self {
            yaml_config_repository,
        }
    }
}

impl<Y> EffectiveConfigLoaderBuilder for DefaultEffectiveConfigLoaderBuilder<Y>
where
    Y: YAMLConfigRepository,
{
    type Loader = EffectiveConfigLoaderImpl<Y>;

    fn build(&self, agent_id: AgentID) -> Self::Loader {
        if agent_id.is_agent_control_id() {
            return EffectiveConfigLoaderImpl::AgentControl(
                AgentControlEffectiveConfigLoader::new(self.yaml_config_repository.clone()),
            );
        }
        EffectiveConfigLoaderImpl::SubAgent(SubAgentEffectiveConfigLoader::new(
            agent_id,
            self.yaml_config_repository.clone(),
        ))
    }
}

/// Enumerates all implementations for `EffectiveConfigLoader` for static dispatching reasons.
pub enum EffectiveConfigLoaderImpl<Y>
where
    Y: YAMLConfigRepository,
{
    AgentControl(AgentControlEffectiveConfigLoader<Y>),
    SubAgent(SubAgentEffectiveConfigLoader<Y>),
}

impl<Y> EffectiveConfigLoader for EffectiveConfigLoaderImpl<Y>
where
    Y: YAMLConfigRepository,
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

    use crate::values::yaml_config_repository::tests::MockYAMLConfigRepository;

    use super::*;

    mock!(
        pub EffectiveConfigLoader {}

        impl EffectiveConfigLoader for EffectiveConfigLoader {
            fn load(&self) -> Result<ConfigurationMap, LoaderError>;
        }
    );

    mock! {
        pub EffectiveConfigLoaderBuilder {}

        impl EffectiveConfigLoaderBuilder for EffectiveConfigLoaderBuilder {
            type Loader = MockEffectiveConfigLoader;

            fn build(&self,agent_id: AgentID) -> MockEffectiveConfigLoader;
        }
    }
    #[test]
    fn builder() {
        let builder =
            DefaultEffectiveConfigLoaderBuilder::new(Arc::new(MockYAMLConfigRepository::default()));

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
