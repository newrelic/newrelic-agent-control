use crate::super_agent::config::{
    SuperAgentConfig, SuperAgentConfigError, SuperAgentDynamicConfig,
};

/// SuperAgentConfigLoader loads a whole SuperAgentConfig
#[cfg_attr(test, mockall::automock)]
pub trait SuperAgentConfigLoader {
    fn load(&self) -> Result<SuperAgentConfig, SuperAgentConfigError>;
}

/// SuperAgentDynamicConfigStorer stores the dynamic part of the SuperAgentConfig
pub trait SuperAgentDynamicConfigStorer {
    fn store(&self, config: &SuperAgentDynamicConfig) -> Result<(), SuperAgentConfigError>;
}

/// SuperAgentDynamicConfigLoaderMock loads the dynamic part of the SuperAgentConfig
#[cfg_attr(test, mockall::automock)]
pub trait SuperAgentDynamicConfigLoader {
    fn load(&self) -> Result<SuperAgentDynamicConfig, SuperAgentConfigError>;
}

/// SuperAgentDynamicConfigStorer deletes the dynamic part of the SuperAgentConfig
pub trait SuperAgentDynamicConfigDeleter {
    fn delete(&self) -> Result<(), SuperAgentConfigError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::SuperAgentConfigError;
    use super::{
        SuperAgentDynamicConfigDeleter, SuperAgentDynamicConfigLoader,
        SuperAgentDynamicConfigStorer,
    };
    use crate::super_agent::config::SuperAgentDynamicConfig;
    use mockall::{mock, predicate};

    mock! {
        pub SuperAgentDynamicConfigStore {}

        impl SuperAgentDynamicConfigStorer for SuperAgentDynamicConfigStore {
            fn store(&self, config: &SuperAgentDynamicConfig) -> Result<(), SuperAgentConfigError>;
        }
        impl SuperAgentDynamicConfigLoader for SuperAgentDynamicConfigStore {
            fn load(&self) -> Result<SuperAgentDynamicConfig, SuperAgentConfigError>;
        }
        impl SuperAgentDynamicConfigDeleter for SuperAgentDynamicConfigStore {
            fn delete(&self) -> Result<(), SuperAgentConfigError>;
        }
    }

    impl MockSuperAgentDynamicConfigStore {
        pub fn should_load(&mut self, sub_agents_config: &SuperAgentDynamicConfig) {
            let sub_agents_config = sub_agents_config.clone();
            self.expect_load()
                .once()
                .returning(move || Ok(sub_agents_config.clone()));
        }

        pub fn should_store(&mut self, sub_agents_config: &SuperAgentDynamicConfig) {
            let sub_agents_config = sub_agents_config.clone();
            self.expect_store()
                .once()
                .with(predicate::eq(sub_agents_config))
                .returning(move |_| Ok(()));
        }
    }
}
