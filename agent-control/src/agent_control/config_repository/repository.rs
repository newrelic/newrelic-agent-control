use crate::agent_control::config::{
    AgentControlConfig, AgentControlConfigError, AgentControlDynamicConfig,
};
use crate::opamp::remote_config::hash::ConfigState;
use crate::values::config::RemoteConfig;

/// AgentControlConfigLoader loads a whole AgentControlConfig
#[cfg_attr(test, mockall::automock)]
pub trait AgentControlConfigLoader {
    fn load(&self) -> Result<AgentControlConfig, AgentControlConfigError>;
}

/// AgentControlDynamicConfigRepository loads, stores, deletes or updates agent_control's remote_configs
#[cfg_attr(test, mockall::automock)]
pub trait AgentControlDynamicConfigRepository {
    /// load the dynamic part of the AgentControlConfig
    fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError>;
    /// store a remote_config containing
    /// the dynamic part of the AgentControlConfig and the remote config hash and status
    fn store(&self, config: &RemoteConfig) -> Result<(), AgentControlConfigError>;

    /// update the state of a remote_config
    fn update_state(&self, state: ConfigState) -> Result<(), AgentControlConfigError>;

    /// retrieves the remote_config if exists
    fn get_remote_config(&self) -> Result<Option<RemoteConfig>, AgentControlConfigError>;

    /// delete the dynamic part of the AgentControlConfig
    fn delete(&self) -> Result<(), AgentControlConfigError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{AgentControlConfigError, AgentControlDynamicConfigRepository};
    use crate::agent_control::agent_id::AgentID;
    use crate::opamp::remote_config::hash::ConfigState;
    use crate::values::config::RemoteConfig;
    use crate::values::config_repository::ConfigRepository;
    use crate::{
        agent_control::config::AgentControlDynamicConfig,
        values::config_repository::tests::InMemoryConfigRepository,
    };
    use mockall::mock;
    use opamp_client::operation::capabilities::Capabilities;

    mock! {
        pub AgentControlDynamicConfigStore {}

        impl AgentControlDynamicConfigRepository for AgentControlDynamicConfigStore {
            fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError>;

            fn store(&self, config: &RemoteConfig) -> Result<(), AgentControlConfigError>;

            fn update_state(&self, state: ConfigState) -> Result<(), AgentControlConfigError>;

            fn get_remote_config(&self) -> Result<Option<RemoteConfig>, AgentControlConfigError>;

            fn delete(&self) -> Result<(), AgentControlConfigError>;
        }
    }

    /// InMemory implementation of [AgentControlDynamicConfigRepository] to be used in unit-tests.
    #[derive(Debug, Default)]
    pub struct InMemoryAgentControlDynamicConfigRepository {
        pub(crate) values_repository: InMemoryConfigRepository,
    }

    impl AgentControlDynamicConfigRepository for InMemoryAgentControlDynamicConfigRepository {
        fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError> {
            let config = self
                .values_repository
                .load_remote_fallback_local(&AgentID::AgentControl, &Capabilities::default())
                .map_err(|e| {
                    AgentControlConfigError(format!("loading Agent Control config: {e}"))
                })?;
            config
                .unwrap_or_default()
                .get_yaml_config()
                .to_owned()
                .try_into()
        }

        fn store(&self, config: &RemoteConfig) -> Result<(), AgentControlConfigError> {
            self.values_repository
                .store_remote(&AgentID::AgentControl, config)
                .map_err(|e| AgentControlConfigError(e.to_string()))
        }

        fn update_state(&self, state: ConfigState) -> Result<(), AgentControlConfigError> {
            self.values_repository
                .update_state(&AgentID::AgentControl, state)
                .map_err(|e| AgentControlConfigError(e.to_string()))
        }

        fn get_remote_config(&self) -> Result<Option<RemoteConfig>, AgentControlConfigError> {
            self.values_repository
                .get_remote_config(&AgentID::AgentControl)
                .map_err(|e| AgentControlConfigError(e.to_string()))
        }

        fn delete(&self) -> Result<(), AgentControlConfigError> {
            self.values_repository
                .delete_remote(&AgentID::AgentControl)
                .map_err(|e| AgentControlConfigError(e.to_string()))
        }
    }
}
