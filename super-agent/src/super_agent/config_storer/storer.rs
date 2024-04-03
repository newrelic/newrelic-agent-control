use crate::super_agent::config::{SubAgentsConfig, SuperAgentConfig, SuperAgentConfigError};

#[cfg_attr(test, mockall::automock)]
pub trait SuperAgentConfigLoader {
    fn load(&self) -> Result<SuperAgentConfig, SuperAgentConfigError>;
}

pub trait SubAgentsConfigStorer {
    fn store(&self, config: &SubAgentsConfig) -> Result<(), SuperAgentConfigError>;
}
pub trait SubAgentsConfigLoader {
    fn load(&self) -> Result<SubAgentsConfig, SuperAgentConfigError>;
}
pub trait SubAgentsConfigDeleter {
    fn delete(&self) -> Result<(), SuperAgentConfigError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::SuperAgentConfigError;
    use super::{SubAgentsConfigDeleter, SubAgentsConfigLoader, SubAgentsConfigStorer};
    use crate::super_agent::config::SubAgentsConfig;
    use mockall::{mock, predicate};

    mock! {
        pub SubAgentsConfigStore {}

        impl SubAgentsConfigStorer for SubAgentsConfigStore {
            fn store(&self, config: &SubAgentsConfig) -> Result<(), SuperAgentConfigError>;
        }
        impl SubAgentsConfigLoader for SubAgentsConfigStore {
            fn load(&self) -> Result<SubAgentsConfig, SuperAgentConfigError>;
        }
        impl SubAgentsConfigDeleter for SubAgentsConfigStore {
            fn delete(&self) -> Result<(), SuperAgentConfigError>;
        }
    }

    impl MockSubAgentsConfigStore {
        pub fn should_load(&mut self, sub_agents_config: &SubAgentsConfig) {
            let sub_agents_config = sub_agents_config.clone();
            self.expect_load()
                .once()
                .returning(move || Ok(sub_agents_config.clone()));
        }

        pub fn should_store(&mut self, sub_agents_config: &SubAgentsConfig) {
            let sub_agents_config = sub_agents_config.clone();
            self.expect_store()
                .once()
                .with(predicate::eq(sub_agents_config))
                .returning(move |_| Ok(()));
        }
    }
}
