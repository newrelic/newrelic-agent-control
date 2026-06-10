use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, instrument};

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::AGENT_FILESYSTEM_FOLDER_NAME;
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::opamp::instance_id::storer::{InstanceIDStorer, StorerError};
use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};

use super::{ResourceCleaner, ResourceCleanerError};

/// On-host implementation of [`ResourceCleaner`] that cleans up all agent resources:
/// - Fleet data (instance IDs, remote config) via storers
/// - Filesystem directories (ephemeral and persistent files)
pub struct OnHostCleaner<S, C>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
{
    instance_id_storer: Arc<S>,
    config_repo: Arc<C>,
    remote_dir: PathBuf,
}

impl<S, C> OnHostCleaner<S, C>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
{
    pub fn new(instance_id_storer: Arc<S>, config_repo: Arc<C>, remote_dir: PathBuf) -> Self {
        Self {
            instance_id_storer,
            config_repo,
            remote_dir,
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

        if let AgentID::SubAgent(id) = agent_id {
            let agent_fs_dir = self
                .remote_dir
                .join(AGENT_FILESYSTEM_FOLDER_NAME)
                .join(id.as_str());

            if agent_fs_dir.exists() {
                debug!(%agent_id, "Removing agent filesystem directory");
                std::fs::remove_dir_all(&agent_fs_dir).map_err(|e| {
                    ResourceCleanerError(format!(
                        "removing agent filesystem dir {}: {e}",
                        agent_fs_dir.display()
                    ))
                })?;
            }
        }

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
    use tempfile::TempDir;

    fn agent_id(s: &str) -> AgentID {
        AgentID::try_from(s).unwrap()
    }

    fn agent_type() -> AgentTypeID {
        AgentTypeID::try_from("newrelic/com.example.foo:0.0.1").unwrap()
    }

    #[test]
    fn clean_deletes_instance_id_remote_config_and_filesystem() {
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

        // Setup filesystem
        let tmp = TempDir::new().unwrap();
        let remote_dir = tmp.path();

        let AgentID::SubAgent(ref raw_id) = id else {
            panic!()
        };
        let agent_fs = remote_dir
            .join(AGENT_FILESYSTEM_FOLDER_NAME)
            .join(raw_id.as_str());

        std::fs::create_dir_all(agent_fs.join("config")).unwrap();
        std::fs::write(agent_fs.join("config/agent.yaml"), "key: val").unwrap();

        let cleaner = OnHostCleaner::new(
            Arc::new(instance_id_storer),
            Arc::new(config_repo),
            remote_dir.to_path_buf(),
        );

        assert!(cleaner.clean(&id, &agent_type()).is_ok());
        assert!(
            !agent_fs.exists(),
            "agent filesystem directory should be removed"
        );
    }

    #[test]
    fn clean_refuses_agent_control_id() {
        let mut instance_id_storer = MockInstanceIDStorer::new();
        instance_id_storer.expect_delete().never();

        let mut config_repo = MockConfigRepository::new();
        config_repo.expect_delete_remote().never();

        let tmp = TempDir::new().unwrap();
        let cleaner = OnHostCleaner::new(
            Arc::new(instance_id_storer),
            Arc::new(config_repo),
            tmp.path().to_path_buf(),
        );

        let result = cleaner.clean(&AgentID::AgentControl, &agent_type());

        assert!(result.is_err());
    }

    #[test]
    fn clean_succeeds_when_no_filesystem_dir() {
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

        let tmp = TempDir::new().unwrap();

        let cleaner = OnHostCleaner::new(
            Arc::new(instance_id_storer),
            Arc::new(config_repo),
            tmp.path().to_path_buf(),
        );

        assert!(cleaner.clean(&id, &agent_type()).is_ok());
    }
}
