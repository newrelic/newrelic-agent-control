use super::ValuesRepositoryError;
use crate::agent_type::agent_values::AgentValues;
use crate::agent_type::definition::AgentType;
use crate::super_agent::config::AgentID;

pub trait ValuesRepository {
    fn load(
        &self,
        agent_id: &AgentID,
        final_agent: &AgentType,
    ) -> Result<AgentValues, ValuesRepositoryError>;

    fn store_remote(
        &self,
        agent_id: &AgentID,
        agent_values: &AgentValues,
    ) -> Result<(), ValuesRepositoryError>;

    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ValuesRepositoryError>;
}

#[cfg(test)]
pub mod test {
    use crate::agent_type::agent_values::AgentValues;
    use crate::agent_type::definition::AgentType;
    use crate::sub_agent::values::values_repository::{ValuesRepository, ValuesRepositoryError};
    use crate::super_agent::config::AgentID;
    use mockall::{mock, predicate};

    mock! {
        pub(crate) RemoteValuesRepositoryMock {}

        impl ValuesRepository for RemoteValuesRepositoryMock {
            fn store_remote(
                &self,
                agent_id: &AgentID,
                agent_values: &AgentValues,
            ) -> Result<(), ValuesRepositoryError> ;
             fn load(
                &self,
                agent_id: &AgentID,
                final_agent: &AgentType,
            ) -> Result<AgentValues, ValuesRepositoryError>;
            fn delete_remote(&self, agent_id: &AgentID) -> Result<(), ValuesRepositoryError>;
        }
    }

    impl MockRemoteValuesRepositoryMock {
        pub fn should_load(
            &mut self,
            agent_id: &AgentID,
            final_agent: &AgentType,
            agent_values: &AgentValues,
        ) {
            let agent_values = agent_values.clone();
            self.expect_load()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(final_agent.clone()),
                )
                .returning(move |_, _| Ok(agent_values.clone()));
        }

        pub fn should_not_load(&mut self, agent_id: &AgentID, final_agent: &AgentType) {
            self.expect_load()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(final_agent.clone()),
                )
                .returning(move |_, _| {
                    Err(ValuesRepositoryError::StoreSerializeError(
                        serde_yaml::from_str::<AgentID>("%---wrong )_$#").unwrap_err(),
                    ))
                });
        }

        pub fn should_store_remote(&mut self, agent_id: &AgentID, agent_values: &AgentValues) {
            self.expect_store_remote()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(agent_values.clone()),
                )
                .returning(|_, _| Ok(()));
        }

        pub fn should_delete_remote(&mut self, agent_id: &AgentID) {
            self.expect_delete_remote()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .returning(|_| Ok(()));
        }
    }
}
