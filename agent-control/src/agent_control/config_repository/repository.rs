use crate::agent_control::config::{
    AgentControlConfig, AgentControlConfigError, AgentControlDynamicConfig,
};
use crate::opamp::remote_config::hash::{ConfigState, Hash};
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
    fn update_hash_state(&self, state: &ConfigState) -> Result<(), AgentControlConfigError>;

    /// retrieves the hash and status from the stored remote_config if exists
    fn get_hash(&self) -> Result<Option<Hash>, AgentControlConfigError>;

    /// delete the dynamic part of the AgentControlConfig
    fn delete(&self) -> Result<(), AgentControlConfigError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{AgentControlConfigError, AgentControlDynamicConfigRepository};
    use crate::agent_control::config::AgentControlDynamicConfig;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::values::config::RemoteConfig;
    use mockall::{mock, predicate};

    mock! {
        pub AgentControlDynamicConfigStore {}

        impl AgentControlDynamicConfigRepository for AgentControlDynamicConfigStore {
            fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError>;

            fn store(&self, config: &RemoteConfig) -> Result<(), AgentControlConfigError>;

            fn update_hash_state(&self, state: &ConfigState) -> Result<(), AgentControlConfigError>;

            fn get_hash(&self) -> Result<Option<Hash>, AgentControlConfigError>;

            fn delete(&self) -> Result<(), AgentControlConfigError>;
        }
    }

    impl MockAgentControlDynamicConfigStore {
        pub fn should_load(&mut self, sub_agents_config: &AgentControlDynamicConfig) {
            let sub_agents_config = sub_agents_config.clone();
            self.expect_load()
                .once()
                .returning(move || Ok(sub_agents_config.clone()));
        }

        pub fn should_store(&mut self, remote_config: RemoteConfig) {
            self.expect_store()
                .once()
                .with(predicate::eq(remote_config))
                .returning(move |_| Ok(()));
        }
    }
}
