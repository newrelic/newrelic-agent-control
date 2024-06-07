use crate::opamp::remote_config_hash::Hash;
use crate::super_agent::config::AgentID;

#[derive(thiserror::Error, Debug)]
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
pub mod test {
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
        #[allow(dead_code)]
        pub fn should_get_hash(&mut self, agent_id: &AgentID, hash: Hash) {
            self.expect_get()
                .with(predicate::eq(agent_id.clone()))
                .once()
                .returning(move |_| Ok(Some(hash.clone())));
        }
        pub fn should_save_hash(&mut self, agent_id: &AgentID, hash: &Hash) {
            self.expect_save()
                .with(predicate::eq(agent_id.clone()), predicate::eq(hash.clone()))
                .once()
                .returning(move |_, _| Ok(()));
        }
    }
}
