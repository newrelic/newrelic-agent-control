use crate::agent_control::config::{
    AgentControlConfig, AgentControlConfigError, AgentControlDynamicConfig,
};
use crate::values::yaml_config::YAMLConfig;

/// AgentControlConfigLoader loads a whole AgentControlConfig
#[cfg_attr(test, mockall::automock)]
pub trait AgentControlConfigLoader {
    fn load(&self) -> Result<AgentControlConfig, AgentControlConfigError>;
}

/// AgentControlDynamicConfigStorer stores the dynamic part of the AgentControlConfig
pub trait AgentControlDynamicConfigStorer {
    fn store(&self, config: &YAMLConfig) -> Result<(), AgentControlConfigError>;
}

/// AgentControlDynamicConfigLoader loads the dynamic part of the AgentControlConfig
#[cfg_attr(test, mockall::automock)]
pub trait AgentControlDynamicConfigLoader {
    fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError>;
}

/// AgentControlDynamicConfigDeleter deletes the dynamic part of the AgentControlConfig
pub trait AgentControlDynamicConfigDeleter {
    fn delete(&self) -> Result<(), AgentControlConfigError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::AgentControlConfigError;
    use super::{
        AgentControlDynamicConfigDeleter, AgentControlDynamicConfigLoader,
        AgentControlDynamicConfigStorer,
    };
    use crate::agent_control::config::AgentControlDynamicConfig;
    use crate::values::yaml_config::YAMLConfig;
    use mockall::{mock, predicate};

    mock! {
        pub AgentControlDynamicConfigStore {}

        impl AgentControlDynamicConfigStorer for AgentControlDynamicConfigStore {
            fn store(&self, config: &YAMLConfig) -> Result<(), AgentControlConfigError>;
        }
        impl AgentControlDynamicConfigLoader for AgentControlDynamicConfigStore {
            fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError>;
        }
        impl AgentControlDynamicConfigDeleter for AgentControlDynamicConfigStore {
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

        pub fn should_store(&mut self, sub_agents_config: &AgentControlDynamicConfig) {
            let sub_agents_config: YAMLConfig = sub_agents_config.try_into().unwrap();
            self.expect_store()
                .once()
                .with(predicate::eq(sub_agents_config))
                .returning(move |_| Ok(()));
        }
    }
}
