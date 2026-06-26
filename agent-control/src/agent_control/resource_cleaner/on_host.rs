use fs::directory_manager::DirectoryManager;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;
use tracing::{debug, instrument};

use crate::agent_control::agent_id::AgentID;
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::opamp::instance_id::storer::{InstanceIDStorer, StorerError};
use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};

use super::{ResourceCleaner, ResourceCleanerError};

/// On-host implementation of [`ResourceCleaner`] that wipes a sub-agent's fleet data by
/// delegating to the same storers that wrote it, also recursively deletes the sub-agent's
/// dedicated filesystem directory, the `persistent` flag is bypassed here because the agent
/// has been removed from the fleet.
pub struct OnHostCleaner<S, C, D>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
    D: DirectoryManager,
{
    instance_id_storer: Arc<S>,
    config_repo: Arc<C>,
    agent_filesystem_base: PathBuf,
    dir_manager: Arc<D>,
}

impl<S, C, D> OnHostCleaner<S, C, D>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
    D: DirectoryManager,
{
    pub fn new(
        instance_id_storer: Arc<S>,
        config_repo: Arc<C>,
        agent_filesystem_base: PathBuf,
        dir_manager: Arc<D>,
    ) -> Self {
        Self {
            instance_id_storer,
            config_repo,
            agent_filesystem_base,
            dir_manager,
        }
    }
}

impl<S, C, D> ResourceCleaner for OnHostCleaner<S, C, D>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
    D: DirectoryManager,
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

        let fs_dir = self.agent_filesystem_base.join(agent_id);
        debug!(%agent_id, path = ?fs_dir, "Cleaning agent filesystem directory");
        self.dir_manager
            .delete(&fs_dir)
            .map_err(|err| OnHostCleanerError::Filesystem {
                path: fs_dir,
                source: err,
            })?;

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
    #[error("failed to delete agent filesystem directory {path:?}: {source}")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl From<OnHostCleanerError> for ResourceCleanerError {
    fn from(err: OnHostCleanerError) -> Self {
        Self(err.to_string())
    }
}

/// Removes filesystem directories under `agent_filesystem_base` whose name does not match a
/// currently-configured agent ID. This handles "agent removed from fleet config while Agent
/// Control was stopped": on next start, the orphaned directory is reclaimed.
pub fn purge_stale_agent_filesystems<'a>(
    dir_manager: &impl DirectoryManager,
    agent_filesystem_base: &std::path::Path,
    configured_agent_ids: impl IntoIterator<Item = &'a str>,
) {
    let configured: HashSet<&str> = configured_agent_ids.into_iter().collect();

    let entries = match dir_manager.list(agent_filesystem_base) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::warn!(
                ?err,
                path = ?agent_filesystem_base,
                "skipping stale-agent filesystem cleanup: cannot read base dir"
            );
            return;
        }
    };

    for path in entries {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if configured.contains(name) {
            continue;
        }
        tracing::info!(
            path = ?path,
            "purging filesystem directory of agent no longer in fleet config"
        );
        if let Err(err) = dir_manager.delete(&path) {
            tracing::warn!(?err, ?path, "failed to purge stale agent filesystem dir");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opamp::instance_id::storer::tests::MockInstanceIDStorer;
    use crate::values::config_repository::tests::MockConfigRepository;
    use ::fs::directory_manager::mock::MockDirectoryManager;
    use mockall::predicate;
    use std::path::Path;

    fn agent_id(s: &str) -> AgentID {
        AgentID::try_from(s).unwrap()
    }

    fn any_type_id() -> AgentTypeID {
        AgentTypeID::try_from("newrelic/com.example.foo:0.0.1").unwrap()
    }

    fn fs_base() -> PathBuf {
        PathBuf::from("/var/lib/newrelic-agent-control/filesystem")
    }

    #[test]
    fn clean_deletes_instance_id_remote_config_and_agent_filesystem_dir() {
        let id = agent_id("foo-agent");
        let expected_fs_dir = fs_base().join(id.as_str());

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

        let mut dir_manager = MockDirectoryManager::new();
        dir_manager.should_delete(&expected_fs_dir);

        let cleaner = OnHostCleaner::new(
            Arc::new(instance_id_storer),
            Arc::new(config_repo),
            fs_base(),
            Arc::new(dir_manager),
        );

        assert!(cleaner.clean(&id, &any_type_id()).is_ok());
    }

    /// When the directory manager's `delete` fails, `clean` propagates the error rather than
    /// swallowing it, annotating it with `agent filesystem directory` for context.
    #[test]
    fn clean_propagates_directory_manager_delete_error() {
        let id = agent_id("foo-agent");
        let expected_fs_dir = fs_base().join(id.as_str());

        let mut instance_id_storer = MockInstanceIDStorer::new();
        instance_id_storer.expect_delete().returning(|_| Ok(()));
        let mut config_repo = MockConfigRepository::new();
        config_repo.expect_delete_remote().returning(|_| Ok(()));
        let mut dir_manager = MockDirectoryManager::new();
        dir_manager.should_not_delete(
            &expected_fs_dir,
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        );

        let cleaner = OnHostCleaner::new(
            Arc::new(instance_id_storer),
            Arc::new(config_repo),
            fs_base(),
            Arc::new(dir_manager),
        );

        let err = cleaner.clean(&id, &any_type_id()).unwrap_err();
        assert!(err.0.contains("agent filesystem directory"));
    }

    #[test]
    fn clean_refuses_agent_control_id() {
        let mut instance_id_storer = MockInstanceIDStorer::new();
        instance_id_storer.expect_delete().never();
        let mut config_repo = MockConfigRepository::new();
        config_repo.expect_delete_remote().never();
        let mut dir_manager = MockDirectoryManager::new();
        dir_manager.expect_delete().never();

        let cleaner = OnHostCleaner::new(
            Arc::new(instance_id_storer),
            Arc::new(config_repo),
            PathBuf::new(),
            Arc::new(dir_manager),
        );

        let result = cleaner.clean(&AgentID::AgentControl, &any_type_id());

        assert!(result.is_err());
    }

    /// Subdirs under the filesystem base whose names are not in the configured-agents set are
    /// removed; configured ones survive.
    #[test]
    fn purge_stale_agent_filesystems_removes_only_orphans() {
        let kept_path = fs_base().join("kept");
        let orphan_path = fs_base().join("orphan");

        let mut dir_manager = MockDirectoryManager::new();
        dir_manager.should_list(&fs_base(), vec![kept_path.clone(), orphan_path.clone()]);
        dir_manager.should_delete(&orphan_path);
        // `kept` is in the configured set → no `delete` call expected for it.

        purge_stale_agent_filesystems(&dir_manager, &fs_base(), ["kept"]);
    }

    #[test]
    fn purge_stale_agent_filesystems_is_noop_on_empty_base() {
        let mut dir_manager = MockDirectoryManager::new();
        dir_manager.should_list(&fs_base(), vec![]);
        dir_manager.expect_delete().never();

        purge_stale_agent_filesystems(&dir_manager, &fs_base(), ["any"]);
    }

    /// If listing the base dir fails, the helper logs and returns without attempting any
    /// deletes. Stale dirs survive until the next AC start.
    #[test]
    fn purge_stale_agent_filesystems_skips_when_list_fails() {
        let mut dir_manager = MockDirectoryManager::new();
        dir_manager
            .expect_list()
            .with(predicate::eq(fs_base()))
            .return_once(|_: &Path| Err(std::io::Error::other("boom")));
        dir_manager.expect_delete().never();

        purge_stale_agent_filesystems(&dir_manager, &fs_base(), ["any"]);
    }
}
