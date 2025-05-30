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

/// AgentControlRemoteConfigStorer stores a remote_config containing
/// the dynamic part of the AgentControlConfig and the remote config hash and status
pub trait AgentControlRemoteConfigStorer {
    fn store(&self, config: &RemoteConfig) -> Result<(), AgentControlConfigError>;
}

/// AgentControlRemoteConfigHashUpdater stores the hash and status for a remote_config
pub trait AgentControlRemoteConfigHashStateUpdater {
    fn update_hash_state(&self, state: &ConfigState) -> Result<(), AgentControlConfigError>;
}

/// AgentControlRemoteConfigHashGetter retrieves the hash and status
/// from the stored remote_config if exists
pub trait AgentControlRemoteConfigHashGetter {
    fn get_hash(&self) -> Result<Option<Hash>, AgentControlConfigError>;
}

/// AgentControlRemoteConfigDeleter deletes the dynamic part of the AgentControlConfig
pub trait AgentControlRemoteConfigDeleter {
    fn delete(&self) -> Result<(), AgentControlConfigError>;
}

/// AgentControlDynamicConfigLoader loads the dynamic part of the AgentControlConfig
#[cfg_attr(test, mockall::automock)]
pub trait AgentControlDynamicConfigLoader {
    fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{
        AgentControlConfigError, AgentControlRemoteConfigHashGetter,
        AgentControlRemoteConfigHashStateUpdater,
    };
    use super::{
        AgentControlDynamicConfigLoader, AgentControlRemoteConfigDeleter,
        AgentControlRemoteConfigStorer,
    };
    use crate::agent_control::config::AgentControlDynamicConfig;
    use crate::opamp::remote_config::hash::{ConfigState, Hash};
    use crate::values::config::RemoteConfig;
    use mockall::{mock, predicate};

    mock! {
        pub AgentControlDynamicConfigStore {}

        impl AgentControlRemoteConfigStorer for AgentControlDynamicConfigStore {
            fn store(&self, config: &RemoteConfig) -> Result<(), AgentControlConfigError>;
        }
        impl AgentControlDynamicConfigLoader for AgentControlDynamicConfigStore {
            fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError>;
        }
        impl AgentControlRemoteConfigDeleter for AgentControlDynamicConfigStore {
            fn delete(&self) -> Result<(), AgentControlConfigError>;
        }
        impl AgentControlRemoteConfigHashStateUpdater for AgentControlDynamicConfigStore {
            fn update_hash_state(&self, state: &ConfigState) -> Result<(), AgentControlConfigError>;
        }
        impl AgentControlRemoteConfigHashGetter for AgentControlDynamicConfigStore {
            fn get_hash(&self) -> Result<Option<Hash>, AgentControlConfigError>;
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
