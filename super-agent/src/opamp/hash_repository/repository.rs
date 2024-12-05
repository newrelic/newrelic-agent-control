use crate::opamp::remote_config_hash::Hash;
use crate::super_agent::config::AgentID;

#[derive(thiserror::Error, Debug, Clone)]
pub enum HashRepositoryError {
    #[error("error persisting hash: `{0}`")]
    PersistError(String),
    #[error("error loading hash: `{0}`")]
    LoadError(String),
}

pub trait HashRepository {
    fn save(&self, agent_id: &AgentID, hash: &Hash) -> Result<(), HashRepositoryError>;
    fn get(&self, agent_id: &AgentID) -> Result<Option<Hash>, HashRepositoryError>;
}

#[cfg(test)]
pub mod tests {
    use super::{AgentID, Hash, HashRepository, HashRepositoryError};
    use mockall::{mock, predicate};

    mock! {
        pub(crate) HashRepositoryMock {}

        impl HashRepository for HashRepositoryMock {

            fn save(&self, agent_id: &AgentID, hash:&Hash) -> Result<(), HashRepositoryError>;

            fn get(&self, agent_id: &AgentID) -> Result<Option<Hash>, HashRepositoryError>;
        }
    }

    impl MockHashRepositoryMock {
        pub fn should_get_hash(&mut self, agent_id: &AgentID, hash: Hash) {
            self.expect_get()
                .with(predicate::eq(agent_id.clone()))
                .once()
                .return_once(move |_| Ok(Some(hash)));
        }

        pub fn should_not_get_hash(&mut self, agent_id: &AgentID) {
            self.expect_get()
                .with(predicate::eq(agent_id.clone()))
                .once()
                .return_once(move |_| Ok(None));
        }

        pub fn should_save_hash(&mut self, agent_id: &AgentID, hash: &Hash) {
            self.expect_save()
                .with(predicate::eq(agent_id.clone()), predicate::eq(hash.clone()))
                .once()
                .returning(move |_, _| Ok(()));
        }

        pub fn should_return_error_on_get(
            &mut self,
            agent_id: &AgentID,
            error: HashRepositoryError,
        ) {
            self.expect_get()
                .with(predicate::eq(agent_id.clone()))
                .once()
                .return_once(move |_| Err(error));
        }
    }
}
