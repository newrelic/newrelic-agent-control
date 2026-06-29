//! Effective-configuration loader trait, builder, and dispatch enum.
use super::agent_control::AgentControlEffectiveConfigLoader;
use super::error::LoaderError;
use super::sub_agent::SubAgentEffectiveConfigLoader;
use crate::agent_control::agent_id::AgentID;
use crate::opamp::remote_config::ConfigurationMap;
use crate::values::config_repository::ConfigRepository;
use std::sync::Arc;

/// Trait for effective configuration loaders.
/// IMPORTANT NOTE: Effective config must be restricted to:
/// - Contain only values that can be modified through opamp remote configs.
/// - Doesn’t contain the real values but the same config defined by users.
///   Meaning no default values should be present.
/// - Doesn’t contain configs that have been set by environment variables.
/// - If a config has an environment variable placeholder, it should be reported as it is.
///   It should never contain the resolved value.
pub trait LoadEffectiveConfig: Send + Sync + 'static {
    /// Load the effective configuration.
    fn load(&self) -> Result<ConfigurationMap, LoaderError>;
}

/// Builds an effective-configuration loader for a given agent.
pub trait BuildEffectiveConfigLoader {
    /// The loader type produced by this builder.
    type Loader: LoadEffectiveConfig;

    /// Builds a loader for the provided agent id.
    fn build(&self, agent_id: AgentID) -> Self::Loader;
}

/// Builder for effective configuration loaders.
pub struct EffectiveConfigLoaderBuilder<Y>
where
    Y: ConfigRepository,
{
    yaml_config_repository: Arc<Y>,
}

impl<Y> EffectiveConfigLoaderBuilder<Y>
where
    Y: ConfigRepository,
{
    /// Creates a builder reading configuration from the given repository.
    pub fn new(yaml_config_repository: Arc<Y>) -> Self {
        Self {
            yaml_config_repository,
        }
    }
}

impl<Y> BuildEffectiveConfigLoader for EffectiveConfigLoaderBuilder<Y>
where
    Y: ConfigRepository,
{
    type Loader = EffectiveConfigLoader<Y>;

    fn build(&self, agent_id: AgentID) -> Self::Loader {
        if agent_id == AgentID::AgentControl {
            return EffectiveConfigLoader::AgentControl(AgentControlEffectiveConfigLoader::new(
                self.yaml_config_repository.clone(),
            ));
        }
        EffectiveConfigLoader::SubAgent(SubAgentEffectiveConfigLoader::new(
            agent_id,
            self.yaml_config_repository.clone(),
        ))
    }
}

/// Enumerates all implementations for `EffectiveConfigLoader` for static dispatching reasons.
pub enum EffectiveConfigLoader<Y>
where
    Y: ConfigRepository,
{
    /// Loader for the agent control's effective configuration.
    AgentControl(AgentControlEffectiveConfigLoader<Y>),
    /// Loader for a sub-agent's effective configuration.
    SubAgent(SubAgentEffectiveConfigLoader<Y>),
}

impl<Y> LoadEffectiveConfig for EffectiveConfigLoader<Y>
where
    Y: ConfigRepository,
{
    fn load(&self) -> Result<ConfigurationMap, LoaderError> {
        match self {
            Self::AgentControl(loader) => loader.load(),
            Self::SubAgent(loader) => loader.load(),
        }
    }
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use mockall::mock;

    use crate::values::config_repository::tests::MockConfigRepository;

    use super::*;

    mock!(
        pub EffectiveConfigLoader {}

        impl LoadEffectiveConfig for EffectiveConfigLoader {
            fn load(&self) -> Result<ConfigurationMap, LoaderError>;
        }
    );

    mock! {
        pub EffectiveConfigLoaderBuilder {}

        impl BuildEffectiveConfigLoader for EffectiveConfigLoaderBuilder {
            type Loader = MockEffectiveConfigLoader;

            fn build(&self,agent_id: AgentID) -> MockEffectiveConfigLoader;
        }
    }
    #[test]
    fn builder() {
        let builder = EffectiveConfigLoaderBuilder::new(Arc::new(MockConfigRepository::default()));

        match builder.build(AgentID::AgentControl) {
            EffectiveConfigLoader::AgentControl(_) => {}
            _ => panic!("Expected AgentControl loader"),
        }

        match builder.build(AgentID::try_from("test").unwrap()) {
            EffectiveConfigLoader::SubAgent(_) => {}
            _ => panic!("Expected SubAgent loader"),
        }
    }
}
