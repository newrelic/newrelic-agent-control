//! On-host resource cleaner that wipes a removed sub-agent's fleet data and OpAMP instance id.

use fs::directory_manager::DirectoryManager;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;
use tracing::{debug, instrument, warn};

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::RESERVED_AGENT_IDS;
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::opamp::instance_id::storer::{InstanceIDStorer, StorerError};
use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};

use super::{ResourceCleaner, ResourceCleanerError};

/// On-host implementation of [`ResourceCleaner`] that wipes a sub-agent's fleet data by
/// delegating to the same storers that wrote it, also recursively deletes the sub-agent's
/// dedicated filesystem directory, the `persistent` flag is bypassed here because the agent
/// has been removed from the fleet.
/// The same removal logic is reused at startup by [`Self::purge_stale_agents`] to reclaim the
/// resources of agents removed from the fleet config while Agent Control was stopped.
pub struct OnHostCleaner<S, C, D>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
    D: DirectoryManager,
{
    instance_id_storer: Arc<S>,
    config_repo: Arc<C>,
    agent_filesystem_base: PathBuf,
    fleet_data_base: PathBuf,
    dir_manager: Arc<D>,
}

impl<S, C, D> OnHostCleaner<S, C, D>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
    D: DirectoryManager,
{
    /// Builds a cleaner delegating to the given instance-id storer and config repository.
    pub fn new(
        instance_id_storer: Arc<S>,
        config_repo: Arc<C>,
        agent_filesystem_base: PathBuf,
        fleet_data_base: PathBuf,
        dir_manager: Arc<D>,
    ) -> Self {
        Self {
            instance_id_storer,
            config_repo,
            agent_filesystem_base,
            fleet_data_base,
            dir_manager,
        }
    }

    /// Deletes all on-disk resources Agent Control owns for `agent_id`: its stored remote config,
    /// its OpAMP instance id, and its dedicated filesystem directory.
    fn remove_agent_resources(&self, agent_id: &AgentID) -> Result<(), OnHostCleanerError> {
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
            })
    }

    /// At startup, reclaims the resources of any agent that is no longer in the agents config.
    pub fn purge_stale_agents<'a>(&self, configured_agent_ids: impl IntoIterator<Item = &'a str>) {
        let mut skip: HashSet<String> = configured_agent_ids.into_iter().map(String::from).collect();
        skip.extend(RESERVED_AGENT_IDS.iter().map(|id| id.to_string()));

        let mut names: HashSet<String> = HashSet::new();
        names.extend(self.agent_dir_names(&self.agent_filesystem_base));
        names.extend(self.agent_dir_names(&self.fleet_data_base));

        for name in names {
            if skip.contains(&name) {
                continue;
            }
            let agent_id = match AgentID::try_from(name.as_str()) {
                Ok(id) => id,
                Err(err) => {
                    warn!(?err, name, "skipping stale directory with invalid agent id");
                    continue;
                }
            };
            tracing::info!(%agent_id, "reclaiming resources of agent no longer in fleet config");
            if let Err(err) = self.remove_agent_resources(&agent_id) {
                warn!(?err, %agent_id, "failed to reclaim stale agent resources");
            }
        }
    }

    /// Lists the immediate child directory names under `base` (the per-agent subdirectories).
    /// A missing `base` yields no names; a listing error is logged and treated as empty.
    fn agent_dir_names(&self, base: &Path) -> Vec<String> {
        match self.dir_manager.list(base) {
            Ok(entries) => entries
                .iter()
                .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
                .collect(),
            Err(err) => {
                warn!(?err, ?base, "cannot list agent directory for stale cleanup");
                Vec::new()
            }
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
        self.remove_agent_resources(agent_id)?;
        Ok(())
    }
}

/// Errors produced by the [`OnHostCleaner`].
#[derive(Debug, Error)]
pub enum OnHostCleanerError {
    /// Cleanup was attempted for the reserved Agent Control id.
    #[error("attempted to clean up resources for Agent Control")]
    AgentControlId,
    /// Failed to delete the stored OpAMP instance id.
    #[error("failed to delete stored instance id: {0}")]
    InstanceId(#[source] StorerError),
    /// Failed to delete the stored remote configuration.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::defaults::AGENT_CONTROL_ID;
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

    fn fleet_base() -> PathBuf {
        PathBuf::from("/var/lib/newrelic-agent-control/fleet-data")
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
            fleet_base(),
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
            fleet_base(),
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
            PathBuf::new(),
            Arc::new(dir_manager),
        );

        let result = cleaner.clean(&AgentID::AgentControl, &any_type_id());

        assert!(result.is_err());
    }

    fn cleaner(
        instance_id_storer: MockInstanceIDStorer,
        config_repo: MockConfigRepository,
        dir_manager: MockDirectoryManager,
    ) -> OnHostCleaner<MockInstanceIDStorer, MockConfigRepository, MockDirectoryManager> {
        OnHostCleaner::new(
            Arc::new(instance_id_storer),
            Arc::new(config_repo),
            fs_base(),
            fleet_base(),
            Arc::new(dir_manager),
        )
    }

    /// Orphans (agents no longer configured) are fully reclaimed — remote config, instance id and
    /// filesystem dir — and discovered from BOTH the filesystem and fleet-data bases. Configured
    /// agents survive.
    #[test]
    fn purge_reclaims_orphans_from_filesystem_and_fleet_data() {
        let orphan_fs = agent_id("orphan-fs");
        let orphan_fleet = agent_id("orphan-fleet");

        let mut dir_manager = MockDirectoryManager::new();
        dir_manager.should_list(
            &fs_base(),
            vec![fs_base().join("kept"), fs_base().join("orphan-fs")],
        );
        dir_manager.should_list(&fleet_base(), vec![fleet_base().join("orphan-fleet")]);
        // `remove_agent_resources` always deletes the agent's filesystem dir (idempotent).
        dir_manager.should_delete(&fs_base().join("orphan-fs"));
        dir_manager.should_delete(&fs_base().join("orphan-fleet"));

        let mut config_repo = MockConfigRepository::new();
        config_repo
            .expect_delete_remote()
            .with(predicate::eq(orphan_fs.clone()))
            .once()
            .returning(|_| Ok(()));
        config_repo
            .expect_delete_remote()
            .with(predicate::eq(orphan_fleet.clone()))
            .once()
            .returning(|_| Ok(()));

        let mut instance_id_storer = MockInstanceIDStorer::new();
        instance_id_storer
            .expect_delete()
            .with(predicate::eq(orphan_fs))
            .once()
            .returning(|_| Ok(()));
        instance_id_storer
            .expect_delete()
            .with(predicate::eq(orphan_fleet))
            .once()
            .returning(|_| Ok(()));

        cleaner(instance_id_storer, config_repo, dir_manager).purge_stale_agents(["kept"]);
    }

    /// Agent Control's own directory (a reserved ID) is never reclaimed.
    #[test]
    fn purge_skips_agent_control_dir() {
        let orphan = agent_id("orphan");

        let mut dir_manager = MockDirectoryManager::new();
        dir_manager.should_list(&fs_base(), vec![]);
        dir_manager.should_list(
            &fleet_base(),
            vec![fleet_base().join(AGENT_CONTROL_ID), fleet_base().join("orphan")],
        );
        dir_manager.should_delete(&fs_base().join("orphan"));

        let mut config_repo = MockConfigRepository::new();
        config_repo
            .expect_delete_remote()
            .with(predicate::eq(orphan.clone()))
            .once()
            .returning(|_| Ok(()));

        let mut instance_id_storer = MockInstanceIDStorer::new();
        instance_id_storer
            .expect_delete()
            .with(predicate::eq(orphan))
            .once()
            .returning(|_| Ok(()));

        cleaner(instance_id_storer, config_repo, dir_manager).purge_stale_agents([]);
    }

    #[test]
    fn purge_is_noop_when_there_are_no_orphans() {
        let mut dir_manager = MockDirectoryManager::new();
        dir_manager.should_list(&fs_base(), vec![]);
        dir_manager.should_list(&fleet_base(), vec![]);
        dir_manager.expect_delete().never();

        let mut config_repo = MockConfigRepository::new();
        config_repo.expect_delete_remote().never();
        let mut instance_id_storer = MockInstanceIDStorer::new();
        instance_id_storer.expect_delete().never();

        cleaner(instance_id_storer, config_repo, dir_manager).purge_stale_agents(["any"]);
    }

    /// If listing one base fails, the helper logs and still reclaims orphans found in the other.
    #[test]
    fn purge_continues_when_one_base_listing_fails() {
        let orphan = agent_id("orphan");

        let mut dir_manager = MockDirectoryManager::new();
        dir_manager
            .expect_list()
            .with(predicate::eq(fs_base()))
            .return_once(|_: &Path| Err(std::io::Error::other("boom")));
        dir_manager.should_list(&fleet_base(), vec![fleet_base().join("orphan")]);
        dir_manager.should_delete(&fs_base().join("orphan"));

        let mut config_repo = MockConfigRepository::new();
        config_repo
            .expect_delete_remote()
            .with(predicate::eq(orphan.clone()))
            .once()
            .returning(|_| Ok(()));
        let mut instance_id_storer = MockInstanceIDStorer::new();
        instance_id_storer
            .expect_delete()
            .with(predicate::eq(orphan))
            .once()
            .returning(|_| Ok(()));

        cleaner(instance_id_storer, config_repo, dir_manager).purge_stale_agents([]);
    }
}
