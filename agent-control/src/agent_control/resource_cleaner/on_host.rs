use std::sync::Arc;

use thiserror::Error;
use tracing::{debug, instrument};

use crate::agent_control::agent_id::AgentID;
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::opamp::instance_id::storer::{InstanceIDStorer, StorerError};
use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};

use super::{ResourceCleaner, ResourceCleanerError};

/// On-host implementation of [`ResourceCleaner`] that wipes a sub-agent's fleet data by
/// delegating to the same storers that wrote it.
pub struct OnHostCleaner<S, C>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
{
    instance_id_storer: Arc<S>,
    config_repo: Arc<C>,
}

impl<S, C> OnHostCleaner<S, C>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
{
    pub fn new(instance_id_storer: Arc<S>, config_repo: Arc<C>) -> Self {
        Self {
            instance_id_storer,
            config_repo,
        }
    }
}

impl<S, C> ResourceCleaner for OnHostCleaner<S, C>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
{
    #[instrument(skip_all, name = "agent_resource_clean", fields(%agent_id))]
    fn clean(
        &self,
        agent_id: &AgentID,
        _agent_type: &AgentTypeID,
    ) -> Result<(), ResourceCleanerError> {
        if agent_id == &AgentID::AgentControl {
            return Err(OnHostCleanerError::AgentControlId.into());
        }

        debug!(%agent_id, "Cleaning remote config data");
        self.config_repo
            .delete_remote(agent_id)
            .map_err(OnHostCleanerError::RemoteConfig)?;

        debug!(%agent_id, "Cleaning opamp identifier data");
        self.instance_id_storer
            .delete(agent_id)
            .map_err(OnHostCleanerError::InstanceId)?;

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum OnHostCleanerError {
    #[error("attempted to clean up resources for Agent Control")]
    AgentControlId,
    #[error("failed to delete stored instance id: {0}")]
    InstanceId(#[source] StorerError),
    #[error("failed to delete stored remote config: {0}")]
    RemoteConfig(#[source] ConfigRepositoryError),
}

impl From<OnHostCleanerError> for ResourceCleanerError {
    fn from(err: OnHostCleanerError) -> Self {
        Self(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opamp::instance_id::storer::tests::MockInstanceIDStorer;
    use crate::values::config_repository::tests::MockConfigRepository;
    use mockall::predicate;

    fn agent_id(s: &str) -> AgentID {
        AgentID::try_from(s).unwrap()
    }

    fn any_type_id() -> AgentTypeID {
        AgentTypeID::try_from("newrelic/com.example.foo:0.0.1").unwrap()
    }

    #[test]
    fn clean_deletes_instance_id_and_remote_config() {
        let id = agent_id("foo-agent");
        let mut instance_id_storer = MockInstanceIDStorer::new();
        instance_id_storer
            .expect_delete()
            .once()
            .with(predicate::eq(id.clone()))
            .returning(|_| Ok(()));

        let mut config_repo = MockConfigRepository::new();
        config_repo
            .expect_delete_remote()
            .once()
            .with(predicate::eq(id.clone()))
            .returning(|_| Ok(()));

        let cleaner = OnHostCleaner::new(Arc::new(instance_id_storer), Arc::new(config_repo));

        assert!(cleaner.clean(&id, &any_type_id()).is_ok());
    }

    #[test]
    fn clean_refuses_agent_control_id() {
        let mut instance_id_storer = MockInstanceIDStorer::new();
        instance_id_storer.expect_delete().never();

        let mut config_repo = MockConfigRepository::new();
        config_repo.expect_delete_remote().never();

        let cleaner = OnHostCleaner::new(Arc::new(instance_id_storer), Arc::new(config_repo));

        let result = cleaner.clean(&AgentID::AgentControl, &any_type_id());

        assert!(result.is_err());
    }
}
