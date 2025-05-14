use crate::agent_control::agent_id::AgentID;
use crate::opamp::remote_config::hash::Hash;

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
        pub(crate) HashRepository {}

        impl HashRepository for HashRepository {

            fn save(&self, agent_id: &AgentID, hash:&Hash) -> Result<(), HashRepositoryError>;

            fn get(&self, agent_id: &AgentID) -> Result<Option<Hash>, HashRepositoryError>;
        }
    }

    impl MockHashRepository {
        pub(crate) fn should_save_hash(&mut self, agent_id: &AgentID, hash: &Hash) {
            self.expect_save()
                .with(predicate::eq(agent_id.clone()), predicate::eq(hash.clone()))
                .once()
                .returning(move |_, _| Ok(()));
        }
    }
}
