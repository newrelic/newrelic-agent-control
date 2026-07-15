//! On-host resource cleaner that wipes a removed sub-agent's fleet data and OpAMP instance id.

use fs::directory_manager::DirectoryManager;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;
use tracing::{debug, instrument, warn};

use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::SubAgentsMap;
use crate::agent_control::defaults::RESERVED_AGENT_IDS;
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::agent_type::registry::{AgentTypeRegistry, AgentTypeRegistryError};
use crate::agent_type::runtime_config::Deployment;
use crate::agent_type::runtime_config::on_host::filesystem::DeclaredPaths;
use crate::opamp::instance_id::storer::{InstanceIDStorer, StorerError};
use crate::package::manager::AgentPackagesRemover;
use crate::package::oci::package_manager::OCIPackageManagerError;
use crate::values::config_repository::{ConfigRepository, ConfigRepositoryError};

use super::{ResourceCleaner, ResourceCleanerError};

/// On-host implementation of [`ResourceCleaner`] that wipes a sub-agent's fleet data by
/// delegating to the same storers that wrote it, also recursively deletes the sub-agent's
/// dedicated filesystem directory and its installed packages (via the [`AgentPackagesRemover`],
/// which owns the on-disk package layout), the `persistent` flag is bypassed here because the
/// agent has been removed from the fleet.
/// The same removal logic is reused at startup by [`Self::cleanup_stale_agents`] to reclaim the
/// resources of agents removed from the fleet config while Agent Control was stopped.
pub struct OnHostCleaner<S, C, D, P, R>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
    D: DirectoryManager,
    P: AgentPackagesRemover,
    R: AgentTypeRegistry,
{
    instance_id_storer: Arc<S>,
    config_repo: Arc<C>,
    agent_filesystem_base: PathBuf,
    fleet_data_base: PathBuf,
    dir_manager: Arc<D>,
    package_remover: Arc<P>,
    registry: Arc<R>,
    shared_filesystem_base: PathBuf,
}

impl<S, C, D, P, R> OnHostCleaner<S, C, D, P, R>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
    D: DirectoryManager,
    P: AgentPackagesRemover,
    R: AgentTypeRegistry,
{
    /// Builds a cleaner delegating to the given instance-id storer, config repository and package
    /// remover, and resolving shared-filesystem paths through the agent type registry.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instance_id_storer: Arc<S>,
        config_repo: Arc<C>,
        agent_filesystem_base: PathBuf,
        fleet_data_base: PathBuf,
        dir_manager: Arc<D>,
        package_remover: Arc<P>,
        registry: Arc<R>,
        shared_filesystem_base: PathBuf,
    ) -> Self {
        Self {
            instance_id_storer,
            config_repo,
            agent_filesystem_base,
            fleet_data_base,
            dir_manager,
            package_remover,
            registry,
            shared_filesystem_base,
        }
    }

    /// Deletes all on-disk resources Agent Control owns for `agent_id`: its stored remote config,
    /// its OpAMP instance id, its dedicated filesystem directory and its installed packages.
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
            })?;

        debug!(%agent_id, "Cleaning agent packages");
        self.package_remover
            .remove(agent_id)
            .map_err(OnHostCleanerError::Packages)?;

        Ok(())
    }

    /// Cleans up everything left behind by agents that are not present in the provided [SubAgentsMap].
    pub fn cleanup_stale_agents(&self, configured: &SubAgentsMap) {
        self.purge_stale_agents(configured.keys().map(|id| id.as_str()));
        self.reconcile_shared_filesystem(configured);
    }

    /// Reclaims the per-agent resources of any agent that is no longer in the agents config.
    fn purge_stale_agents<'a>(&self, configured_agent_ids: impl IntoIterator<Item = &'a str>) {
        let skip: HashSet<String> = configured_agent_ids
            .into_iter()
            .map(String::from)
            .chain(RESERVED_AGENT_IDS.iter().map(|id| id.to_string()))
            .collect();

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
    fn agent_dir_names(&self, base: &Path) -> impl Iterator<Item = String> {
        self.dir_manager
            .list(base)
            .inspect_err(|err| warn!(?err, ?base, "cannot list agent directory for stale cleanup"))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
    }

    /// The shared-filesystem paths an agent type declares (static, read from the type definition).
    /// A non-on-host type declares none; an unresolvable type is an error.
    fn declared_shared_paths(
        &self,
        agent_type: &AgentTypeID,
    ) -> Result<DeclaredPaths, OnHostCleanerError> {
        let definition = self.registry.get(agent_type).map_err(|source| {
            OnHostCleanerError::AgentTypeResolution {
                agent_type: agent_type.clone(),
                source: Box::new(source),
            }
        })?;
        Ok(match &definition.runtime_config.deployment {
            Deployment::Host(on_host) => on_host
                .shared_filesystem()
                .declared_paths(&self.shared_filesystem_base),
            Deployment::K8s(_) => DeclaredPaths::default(),
        })
    }

    /// All per-agent filesystem paths an agent type declares, rooted at the agent's dir.
    fn declared_agent_filesystem_paths(
        &self,
        agent_id: &AgentID,
        agent_type: &AgentTypeID,
    ) -> Result<DeclaredPaths, OnHostCleanerError> {
        let definition = self.registry.get(agent_type).map_err(|source| {
            OnHostCleanerError::AgentTypeResolution {
                agent_type: agent_type.clone(),
                source: Box::new(source),
            }
        })?;
        let agent_dir = self.agent_filesystem_base.join(agent_id.as_str());
        Ok(match &definition.runtime_config.deployment {
            Deployment::Host(on_host) => on_host.filesystem().declared_paths(&agent_dir),
            Deployment::K8s(_) => DeclaredPaths::default(),
        })
    }

    /// Union of shared-filesystem paths declared by every agent in `agents`.
    fn declared_shared_paths_from_all_agents(
        &self,
        agents: &SubAgentsMap,
    ) -> Result<DeclaredPaths, OnHostCleanerError> {
        let mut all_shared_paths = DeclaredPaths::default();
        for config in agents.values() {
            let paths = self.declared_shared_paths(&config.agent_type)?;
            all_shared_paths.files.extend(paths.files);
            all_shared_paths.managed_dirs.extend(paths.managed_dirs);
        }
        Ok(all_shared_paths)
    }

    /// Reconciles both the per-agent and shared filesystems when an agent type changes.
    /// Per-agent filesystem: paths declared by `old_type` but absent in `new_type` are deleted.
    /// Shared filesystem: only paths that no currently active agent still declares are deleted.
    /// This prevents removing paths that are co-owned by another agent.
    fn reconcile_filesystems_on_type_change(
        &self,
        agent_id: &AgentID,
        old_type: &AgentTypeID,
        new_type: &AgentTypeID,
        active_agents: &SubAgentsMap,
    ) -> Result<(), OnHostCleanerError> {
        let old_agent = self.declared_agent_filesystem_paths(agent_id, old_type)?;
        let new_agent = self.declared_agent_filesystem_paths(agent_id, new_type)?;
        delete_stale_paths(&old_agent, &new_agent)
            .map_err(|(path, source)| OnHostCleanerError::AgentFilesystem { path, source })?;

        let old_shared = self.declared_shared_paths(old_type)?;
        let active_shared = self.declared_shared_paths_from_all_agents(active_agents)?;
        delete_stale_paths(&old_shared, &active_shared)
            .map_err(|(path, source)| OnHostCleanerError::SharedFilesystem { path, source })?;

        Ok(())
    }

    /// Removes a removed agent's shared paths: its files and whole managed directories. Co-owned
    /// directories are never declared, so they stay. Errors propagate so a failed cleanup surfaces.
    fn remove_agent_shared_paths(
        &self,
        agent_type: &AgentTypeID,
    ) -> Result<(), OnHostCleanerError> {
        let declared = self.declared_shared_paths(agent_type)?;
        for path in declared.files.iter().chain(&declared.managed_dirs) {
            remove_path(path).map_err(|source| OnHostCleanerError::SharedFilesystem {
                path: path.clone(),
                source,
            })?;
        }
        Ok(())
    }

    /// Deletes shared files no configured agent owns. Directories are not deleted (as can they be co-owned).
    fn reconcile_shared_filesystem(&self, configured: &SubAgentsMap) {
        let mut expected_files = HashSet::new();
        let mut owned_managed_dirs = HashSet::new();
        for agent_config in configured.values() {
            match self.declared_shared_paths(&agent_config.agent_type) {
                Ok(declared) => {
                    expected_files.extend(declared.files);
                    owned_managed_dirs.extend(declared.managed_dirs);
                }
                Err(err) => {
                    warn!(
                        ?err,
                        "skipping shared filesystem reconcile: an agent type could not be resolved"
                    );
                    return;
                }
            }
        }

        if let Err(err) = reconcile_shared_dir(
            &self.shared_filesystem_base,
            &expected_files,
            &owned_managed_dirs,
        ) {
            warn!(?err, base = ?self.shared_filesystem_base, "shared filesystem reconcile failed");
        }
    }
}

/// Deletes paths in `old` that are absent in `new`. Returns the offending `(path, io::Error)` on
/// failure so the caller can wrap it into the appropriate error variant via `map_err`.
fn delete_stale_paths(
    old: &DeclaredPaths,
    new: &DeclaredPaths,
) -> Result<(), (PathBuf, io::Error)> {
    for path in old
        .files
        .difference(&new.files)
        .chain(old.managed_dirs.difference(&new.managed_dirs))
    {
        remove_path(path).map_err(|e| (path.clone(), e))?;
    }
    Ok(())
}

/// Deletes a file or a whole directory subtree at `path`. A missing path is not an error.
fn remove_path(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Recursively deletes files under `dir` not in `expected_files`, keeping all directories. A dir in
/// `owned_managed_dirs` is kept whole (not descended into).
fn reconcile_shared_dir(
    dir: &Path,
    expected_files: &HashSet<PathBuf>,
    owned_managed_dirs: &HashSet<PathBuf>,
) -> io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            if owned_managed_dirs.contains(&path) {
                continue;
            }
            reconcile_shared_dir(&path, expected_files, owned_managed_dirs)?;
        } else if !expected_files.contains(&path) {
            remove_path(&path)?;
        }
    }
    Ok(())
}

impl<S, C, D, P, R> ResourceCleaner for OnHostCleaner<S, C, D, P, R>
where
    S: InstanceIDStorer,
    C: ConfigRepository,
    D: DirectoryManager,
    P: AgentPackagesRemover,
    R: AgentTypeRegistry,
{
    #[instrument(skip_all, name = "agent_resource_clean", fields(%agent_id))]
    fn clean(
        &self,
        agent_id: &AgentID,
        agent_type: &AgentTypeID,
    ) -> Result<(), ResourceCleanerError> {
        if agent_id == &AgentID::AgentControl {
            return Err(OnHostCleanerError::AgentControlId.into());
        }
        self.remove_agent_resources(agent_id)?;
        self.remove_agent_shared_paths(agent_type)?;
        Ok(())
    }

    #[instrument(skip_all, name = "agent_type_change_reconcile", fields(%agent_id))]
    fn on_agent_type_changed(
        &self,
        agent_id: &AgentID,
        old_agent_type: &AgentTypeID,
        new_agent_type: &AgentTypeID,
        active_agents: &SubAgentsMap,
    ) -> Result<(), ResourceCleanerError> {
        self.reconcile_filesystems_on_type_change(
            agent_id,
            old_agent_type,
            new_agent_type,
            active_agents,
        )?;
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
    /// Failed to delete agent filesystem directory.
    #[error("failed to delete agent filesystem directory {path:?}: {source}")]
    Filesystem {
        /// The path in the filesystem that couldn't be deleted.
        path: PathBuf,
        /// The io error.
        #[source]
        source: std::io::Error,
    },
    /// Failed to remove the agent's installed packages.
    #[error("failed to remove agent packages: {0}")]
    Packages(#[source] OCIPackageManagerError),
    /// Failed to delete a per-agent filesystem path owned by the removed or type-changed agent.
    #[error("failed to delete agent filesystem path {path:?}: {source}")]
    AgentFilesystem {
        /// The path that couldn't be deleted.
        path: PathBuf,
        /// The io error.
        #[source]
        source: std::io::Error,
    },
    /// The agent type could not be resolved, so its shared-filesystem paths are unknown.
    #[error("failed to resolve agent type {agent_type} for shared filesystem cleanup: {source}")]
    AgentTypeResolution {
        /// The agent type that could not be resolved.
        agent_type: AgentTypeID,
        /// The registry lookup error (boxed to keep the error enum small).
        #[source]
        source: Box<AgentTypeRegistryError>,
    },
    /// Failed to delete a shared-filesystem path owned by the removed agent.
    #[error("failed to delete shared filesystem path {path:?}: {source}")]
    SharedFilesystem {
        /// The shared path that couldn't be deleted.
        path: PathBuf,
        /// The io error.
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
    use crate::agent_type::definition::AgentTypeDefinition;
    use crate::agent_type::registry::tests::MockAgentTypeRegistry;
    use crate::opamp::instance_id::storer::tests::MockInstanceIDStorer;
    use crate::package::manager::tests::MockAgentPackagesRemover;
    use crate::values::config_repository::tests::MockConfigRepository;
    use ::fs::directory_manager::mock::MockDirectoryManager;
    use mockall::predicate;
    use std::path::Path;

    fn agent_id(s: &str) -> AgentID {
        AgentID::try_from(s).unwrap()
    }

    fn packages_error() -> OCIPackageManagerError {
        OCIPackageManagerError::RemoveAgentPackages {
            path: "/var/lib/newrelic-agent-control/packages/foo-agent".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        }
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

    fn shared_base() -> PathBuf {
        PathBuf::from("/var/lib/newrelic-agent-control/shared-filesystem")
    }

    /// A registry whose types declare no shared filesystem (empty/k8s definitions), so shared-path
    /// cleanup is a no-op. Used by the per-agent cleanup tests, which do not exercise shared paths.
    fn no_shared_registry() -> MockAgentTypeRegistry {
        let mut registry = MockAgentTypeRegistry::new();
        registry
            .expect_get()
            .returning(|id| Ok(AgentTypeDefinition::empty_with_metadata(id.clone())));
        registry
    }

    /// Builds a cleaner for shared-filesystem tests: real registry-driven paths against a real
    /// (temp) shared base, with the per-agent collaborators left unused (shared cleanup does not
    /// touch them).
    fn shared_cleaner(
        registry: MockAgentTypeRegistry,
        shared_base: PathBuf,
    ) -> OnHostCleaner<
        MockInstanceIDStorer,
        MockConfigRepository,
        MockDirectoryManager,
        MockAgentPackagesRemover,
        MockAgentTypeRegistry,
    > {
        OnHostCleaner::new(
            Arc::new(MockInstanceIDStorer::new()),
            Arc::new(MockConfigRepository::new()),
            fs_base(),
            fleet_base(),
            Arc::new(MockDirectoryManager::new()),
            Arc::new(MockAgentPackagesRemover::new()),
            Arc::new(registry),
            shared_base,
        )
    }

    /// A host Agent Type whose `deployment.shared_filesystem` is the given entry tree (built as a
    /// JSON value, so no YAML indentation to align).
    fn host_type_with_filesystem(filesystem: serde_json::Value) -> AgentTypeDefinition {
        let definition = serde_json::json!({
            "name": "ohi",
            "namespace": "test",
            "version": "0.0.1",
            "platform": "host",
            "operating_system": "linux",
            "variables": {},
            "deployment": { "filesystem": filesystem },
        });
        let yaml = serde_saphyr::to_string(&definition).expect("definition must serialize");
        serde_saphyr::from_str(&yaml).expect("host agent type must parse")
    }

    /// Cleaner wired for agent-filesystem reconciliation tests: real filesystem bases against a
    /// real (temp) per-agent dir, with unused collaborators stubbed out.
    fn agent_filesystem_cleaner(
        registry: MockAgentTypeRegistry,
        agent_filesystem_base: PathBuf,
    ) -> OnHostCleaner<
        MockInstanceIDStorer,
        MockConfigRepository,
        MockDirectoryManager,
        MockAgentPackagesRemover,
        MockAgentTypeRegistry,
    > {
        OnHostCleaner::new(
            Arc::new(MockInstanceIDStorer::new()),
            Arc::new(MockConfigRepository::new()),
            agent_filesystem_base,
            fleet_base(),
            Arc::new(MockDirectoryManager::new()),
            Arc::new(MockAgentPackagesRemover::new()),
            Arc::new(registry),
            shared_base(),
        )
    }

    fn host_type_with_shared(shared_filesystem: serde_json::Value) -> AgentTypeDefinition {
        let definition = serde_json::json!({
            "name": "ohi",
            "namespace": "test",
            "version": "0.0.1",
            "platform": "host",
            "operating_system": "linux",
            "variables": {},
            "deployment": { "shared_filesystem": shared_filesystem },
        });
        let yaml = serde_saphyr::to_string(&definition).expect("definition must serialize");
        serde_saphyr::from_str(&yaml).expect("host agent type must parse")
    }

    fn configured(agents: &[(&str, &str)]) -> SubAgentsMap {
        agents
            .iter()
            .map(|(id, agent_type)| {
                (
                    agent_id(id),
                    crate::agent_control::config::SubAgentConfig {
                        agent_type: AgentTypeID::try_from(*agent_type).unwrap(),
                    },
                )
            })
            .collect()
    }

    fn cleaner(
        instance_id_storer: MockInstanceIDStorer,
        config_repo: MockConfigRepository,
        dir_manager: MockDirectoryManager,
        package_remover: MockAgentPackagesRemover,
    ) -> OnHostCleaner<
        MockInstanceIDStorer,
        MockConfigRepository,
        MockDirectoryManager,
        MockAgentPackagesRemover,
        MockAgentTypeRegistry,
    > {
        OnHostCleaner::new(
            Arc::new(instance_id_storer),
            Arc::new(config_repo),
            fs_base(),
            fleet_base(),
            Arc::new(dir_manager),
            Arc::new(package_remover),
            Arc::new(no_shared_registry()),
            shared_base(),
        )
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

        let cleaner = cleaner(
            instance_id_storer,
            config_repo,
            dir_manager,
            MockAgentPackagesRemover::new().removing(&[id.as_str()]),
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

        let cleaner = cleaner(
            instance_id_storer,
            config_repo,
            dir_manager,
            MockAgentPackagesRemover::new().removing(&[]),
        );

        let err = cleaner.clean(&id, &any_type_id()).unwrap_err();
        assert!(err.0.contains("agent filesystem directory"));
    }

    #[test]
    fn clean_propagates_package_removal_error() {
        let id = agent_id("foo-agent");
        let expected_fs_dir = fs_base().join(id.as_str());

        let mut instance_id_storer = MockInstanceIDStorer::new();
        instance_id_storer.expect_delete().returning(|_| Ok(()));
        let mut config_repo = MockConfigRepository::new();
        config_repo.expect_delete_remote().returning(|_| Ok(()));
        let mut dir_manager = MockDirectoryManager::new();
        dir_manager.should_delete(&expected_fs_dir);

        let mut package_remover = MockAgentPackagesRemover::new();
        package_remover
            .expect_remove()
            .with(predicate::eq(id.clone()))
            .once()
            .returning(|_| Err(packages_error()));

        let cleaner = cleaner(
            instance_id_storer,
            config_repo,
            dir_manager,
            package_remover,
        );

        let err = cleaner.clean(&id, &any_type_id()).unwrap_err();
        assert!(err.0.contains("failed to remove agent packages"));
    }

    #[test]
    fn clean_refuses_agent_control_id() {
        let mut instance_id_storer = MockInstanceIDStorer::new();
        instance_id_storer.expect_delete().never();
        let mut config_repo = MockConfigRepository::new();
        config_repo.expect_delete_remote().never();
        let mut dir_manager = MockDirectoryManager::new();
        dir_manager.expect_delete().never();
        let mut package_remover = MockAgentPackagesRemover::new();
        package_remover.expect_remove().never();

        let cleaner = cleaner(
            instance_id_storer,
            config_repo,
            dir_manager,
            package_remover,
        );

        let result = cleaner.clean(&AgentID::AgentControl, &any_type_id());

        assert!(result.is_err());
    }

    /// Orphans (agents no longer configured) are fully reclaimed — remote config, instance id,
    /// filesystem dir and packages — and discovered from BOTH the filesystem and fleet-data bases.
    /// Configured agents survive.
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
        let mut config_repo = MockConfigRepository::new();
        let mut instance_id_storer = MockInstanceIDStorer::new();
        // `remove_agent_resources` always deletes the agent's filesystem dir (idempotent),
        // regardless of which base the orphan was discovered from.
        for orphan in [&orphan_fs, &orphan_fleet] {
            dir_manager.should_delete(&fs_base().join(orphan.as_str()));
            config_repo
                .expect_delete_remote()
                .with(predicate::eq(orphan.clone()))
                .once()
                .returning(|_| Ok(()));
            instance_id_storer
                .expect_delete()
                .with(predicate::eq(orphan.clone()))
                .once()
                .returning(|_| Ok(()));
        }

        let package_remover =
            MockAgentPackagesRemover::new().removing(&["orphan-fs", "orphan-fleet"]);

        cleaner(
            instance_id_storer,
            config_repo,
            dir_manager,
            package_remover,
        )
        .purge_stale_agents(["kept"]);
    }

    /// Agent Control's own directory (a reserved ID) is never reclaimed.
    #[test]
    fn purge_skips_agent_control_dir() {
        let orphan = agent_id("orphan");

        let mut dir_manager = MockDirectoryManager::new();
        dir_manager.should_list(&fs_base(), vec![]);
        dir_manager.should_list(
            &fleet_base(),
            vec![
                fleet_base().join(AGENT_CONTROL_ID),
                fleet_base().join("orphan"),
            ],
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
            .with(predicate::eq(orphan.clone()))
            .once()
            .returning(|_| Ok(()));

        let package_remover = MockAgentPackagesRemover::new().removing(&[orphan.as_str()]);

        cleaner(
            instance_id_storer,
            config_repo,
            dir_manager,
            package_remover,
        )
        .purge_stale_agents([]);
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

        let package_remover = MockAgentPackagesRemover::new().removing(&[]);

        cleaner(
            instance_id_storer,
            config_repo,
            dir_manager,
            package_remover,
        )
        .purge_stale_agents(["any"]);
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
            .with(predicate::eq(orphan.clone()))
            .once()
            .returning(|_| Ok(()));

        let package_remover = MockAgentPackagesRemover::new().removing(&[orphan.as_str()]);

        cleaner(
            instance_id_storer,
            config_repo,
            dir_manager,
            package_remover,
        )
        .purge_stale_agents([]);
    }

    #[test]
    fn remove_agent_shared_paths_deletes_owned_and_keeps_co_owned() {
        let tmp = tempfile::TempDir::new().unwrap();
        let shared = tmp.path().to_path_buf();
        std::fs::create_dir_all(shared.join("ohi-configs")).unwrap();
        std::fs::write(shared.join("ohi-configs").join("nri-redis.yaml"), "redis").unwrap();
        std::fs::write(shared.join("ohi-configs").join("nri-other.yaml"), "other").unwrap();
        std::fs::create_dir_all(shared.join("additional-files")).unwrap();
        std::fs::write(shared.join("additional-files").join("app.log"), "log").unwrap();

        let definition = host_type_with_shared(serde_json::json!({
            "ohi-configs": {
                "kind": "dir",
                "entries": { "nri-redis.yaml": { "kind": "file", "text": "redis" } },
            },
            "additional-files": { "kind": "dir_content_from_map", "source": "${nr-var:ohi_additional_files}" },
        }));
        let type_id = AgentTypeID::try_from("test/ohi:0.0.1").unwrap();
        let mut registry = MockAgentTypeRegistry::new();
        registry.should_get(type_id.clone(), &definition);

        shared_cleaner(registry, shared.clone())
            .remove_agent_shared_paths(&type_id)
            .expect("shared path removal should succeed");

        assert!(
            !shared.join("ohi-configs").join("nri-redis.yaml").exists(),
            "the agent's own file must be removed"
        );
        assert!(
            !shared.join("additional-files").exists(),
            "the agent's whole managed directory must be removed"
        );
        assert!(
            shared.join("ohi-configs").is_dir(),
            "the co-owned drop-zone directory must be kept"
        );
        assert!(
            shared.join("ohi-configs").join("nri-other.yaml").exists(),
            "another agent's sibling file must be kept"
        );
    }

    #[test]
    fn reconcile_deletes_orphans_and_keeps_owned() {
        let tmp = tempfile::TempDir::new().unwrap();
        let shared = tmp.path().to_path_buf();
        std::fs::create_dir_all(shared.join("ohi-configs")).unwrap();
        std::fs::write(shared.join("ohi-configs").join("nri-redis.yaml"), "redis").unwrap();
        std::fs::write(shared.join("ohi-configs").join("nri-orphan.yaml"), "orphan").unwrap();
        std::fs::create_dir_all(shared.join("additional-files")).unwrap();
        std::fs::write(shared.join("additional-files").join("dynamic.log"), "dyn").unwrap();
        std::fs::write(shared.join("top-orphan.txt"), "top").unwrap();

        let definition = host_type_with_shared(serde_json::json!({
            "ohi-configs": {
                "kind": "dir",
                "entries": { "nri-redis.yaml": { "kind": "file", "text": "redis" } },
            },
            "additional-files": { "kind": "dir_content_from_map", "source": "${nr-var:ohi_additional_files}" },
        }));
        let mut registry = MockAgentTypeRegistry::new();
        registry.should_get(
            AgentTypeID::try_from("test/redis:0.0.1").unwrap(),
            &definition,
        );

        shared_cleaner(registry, shared.clone())
            .reconcile_shared_filesystem(&configured(&[("redis-agent", "test/redis:0.0.1")]));

        assert!(
            shared.join("ohi-configs").join("nri-redis.yaml").exists(),
            "a configured agent's file must be kept"
        );
        assert!(
            !shared.join("ohi-configs").join("nri-orphan.yaml").exists(),
            "an orphan file in a co-owned dir must be deleted"
        );
        assert!(
            !shared.join("top-orphan.txt").exists(),
            "a top-level orphan file must be deleted"
        );
        assert!(
            shared.join("additional-files").join("dynamic.log").exists(),
            "contents of a currently-owned managed directory must be kept"
        );
        assert!(
            shared.join("ohi-configs").is_dir(),
            "co-owned directories must never be deleted"
        );
    }

    /// On a type bump all per-agent paths declared by the old type but absent in the new type are
    /// deleted regardless of persistence. Files present in both types are kept.
    #[test]
    fn on_type_change_reconciles_agent_filesystem() {
        let tmp = tempfile::TempDir::new().unwrap();
        let fs_base = tmp.path().to_path_buf();
        let agent = agent_id("my-agent");
        let agent_dir = fs_base.join(agent.as_str());
        std::fs::create_dir_all(&agent_dir).unwrap();

        std::fs::write(agent_dir.join("config.yaml"), "cfg").unwrap();
        std::fs::write(agent_dir.join("removed.yaml"), "old").unwrap();
        std::fs::create_dir_all(agent_dir.join("logging.d")).unwrap();
        std::fs::write(agent_dir.join("logging.d").join("syslog.yaml"), "log").unwrap();
        std::fs::write(agent_dir.join("persistent.db"), "state").unwrap();

        let old_type_id = AgentTypeID::try_from("test/oldagent:0.0.1").unwrap();
        let new_type_id = AgentTypeID::try_from("test/newagent:0.0.2").unwrap();

        let old_definition = host_type_with_filesystem(serde_json::json!({
            "config.yaml":  { "kind": "file", "text": "cfg" },
            "removed.yaml": { "kind": "file", "text": "old" },
            "logging.d":    { "kind": "dir_content_from_map", "source": "${nr-var:logs}" },
            "persistent.db": { "kind": "file", "text": "state", "persistent": true },
        }));
        let new_definition = host_type_with_filesystem(serde_json::json!({
            "config.yaml": { "kind": "file", "text": "cfg" },
        }));

        let mut registry = MockAgentTypeRegistry::new();
        // Two lookups per type: one for agent filesystem, one for shared filesystem.
        registry.should_get(old_type_id.clone(), &old_definition);
        registry.should_get(old_type_id.clone(), &old_definition);
        registry.should_get(new_type_id.clone(), &new_definition);
        registry.should_get(new_type_id.clone(), &new_definition);

        // active_agents contains this agent at its new type, mirroring what production passes.
        let active = configured(&[(agent.as_str(), "test/newagent:0.0.2")]);
        agent_filesystem_cleaner(registry, fs_base.clone())
            .reconcile_filesystems_on_type_change(&agent, &old_type_id, &new_type_id, &active)
            .expect("reconcile must succeed");

        assert!(
            agent_dir.join("config.yaml").exists(),
            "file present in both types must be kept"
        );
        assert!(
            !agent_dir.join("removed.yaml").exists(),
            "ephemeral file absent in new type must be deleted"
        );
        assert!(
            !agent_dir.join("logging.d").exists(),
            "managed dir absent in new type must be deleted"
        );
        assert!(
            !agent_dir.join("persistent.db").exists(),
            "persistent file absent in new type must be deleted"
        );
    }

    /// On a type bump, shared files present in the old type but absent in the new type are deleted.
    /// Files present in both are kept. Managed dirs absent in the new type are removed.
    #[test]
    fn on_type_change_removes_ephemeral_paths_absent_in_new_type() {
        let tmp = tempfile::TempDir::new().unwrap();
        let shared = tmp.path().to_path_buf();

        // On-disk state from the old type.
        std::fs::create_dir_all(shared.join("ohi-configs")).unwrap();
        std::fs::write(shared.join("ohi-configs").join("nri-redis.yaml"), "redis").unwrap();
        std::fs::write(shared.join("ohi-configs").join("nri-old.yaml"), "old").unwrap();
        std::fs::create_dir_all(shared.join("old-managed")).unwrap();
        std::fs::write(shared.join("old-managed").join("file.log"), "log").unwrap();

        let old_type_id = AgentTypeID::try_from("test/oldagent:0.0.1").unwrap();
        let new_type_id = AgentTypeID::try_from("test/newagent:0.0.2").unwrap();

        // Old type declares: redis.yaml (ephemeral file), nri-old.yaml (ephemeral file), old-managed (DirContentFromMap).
        let old_definition = host_type_with_shared(serde_json::json!({
            "ohi-configs": {
                "kind": "dir",
                "entries": {
                    "nri-redis.yaml": { "kind": "file", "text": "redis" },
                    "nri-old.yaml":   { "kind": "file", "text": "old"   },
                },
            },
            "old-managed": { "kind": "dir_content_from_map", "source": "${nr-var:m}" },
        }));

        // New type declares: redis.yaml only (same file, kept). nri-old.yaml and old-managed are gone.
        let new_definition = host_type_with_shared(serde_json::json!({
            "ohi-configs": {
                "kind": "dir",
                "entries": {
                    "nri-redis.yaml": { "kind": "file", "text": "redis" },
                },
            },
        }));

        let mut registry = MockAgentTypeRegistry::new();
        // Two lookups per type: one for agent filesystem, one for shared filesystem.
        registry.should_get(old_type_id.clone(), &old_definition);
        registry.should_get(old_type_id.clone(), &old_definition);
        registry.should_get(new_type_id.clone(), &new_definition);
        registry.should_get(new_type_id.clone(), &new_definition);

        let result = shared_cleaner(registry, shared.clone()).reconcile_filesystems_on_type_change(
            &agent_id("test-agent"),
            &old_type_id,
            &new_type_id,
            &configured(&[("test-agent", "test/newagent:0.0.2")]),
        );

        assert!(result.is_ok(), "reconcile must succeed: {result:?}");
        assert!(
            shared.join("ohi-configs").join("nri-redis.yaml").exists(),
            "file present in both types must be kept"
        );
        assert!(
            !shared.join("ohi-configs").join("nri-old.yaml").exists(),
            "file in old type but absent in new type must be deleted"
        );
        assert!(
            !shared.join("old-managed").exists(),
            "managed dir present in old type but absent in new type must be deleted"
        );
        assert!(
            shared.join("ohi-configs").is_dir(),
            "co-owned directory must never be deleted"
        );
    }

    /// All shared paths declared by the old type but absent in the new type are deleted, including
    /// persistent ones.
    #[test]
    fn on_type_change_removes_paths_absent_in_new_type_including_persistent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let shared = tmp.path().to_path_buf();

        std::fs::write(shared.join("persistent.yaml"), "data").unwrap();
        std::fs::write(shared.join("ephemeral.yaml"), "tmp").unwrap();

        let old_type_id = AgentTypeID::try_from("test/oldagent:0.0.1").unwrap();
        let new_type_id = AgentTypeID::try_from("test/newagent:0.0.2").unwrap();

        let old_definition = host_type_with_shared(serde_json::json!({
            "persistent.yaml": { "kind": "file", "text": "data", "persistent": true },
            "ephemeral.yaml":  { "kind": "file", "text": "tmp"  },
        }));
        let new_definition = host_type_with_shared(serde_json::json!({}));

        let mut registry = MockAgentTypeRegistry::new();
        // Two lookups per type: one for agent filesystem, one for shared filesystem.
        registry.should_get(old_type_id.clone(), &old_definition);
        registry.should_get(old_type_id.clone(), &old_definition);
        registry.should_get(new_type_id.clone(), &new_definition);
        registry.should_get(new_type_id.clone(), &new_definition);

        shared_cleaner(registry, shared.clone())
            .reconcile_filesystems_on_type_change(
                &agent_id("test-agent"),
                &old_type_id,
                &new_type_id,
                &configured(&[("test-agent", "test/newagent:0.0.2")]),
            )
            .expect("reconcile must succeed");

        assert!(
            !shared.join("persistent.yaml").exists(),
            "persistent file absent in new type must be deleted"
        );
        assert!(
            !shared.join("ephemeral.yaml").exists(),
            "ephemeral file absent in new type must be deleted"
        );
    }

    /// A shared path dropped by the type-bumped agent must NOT be deleted if another active agent
    /// still declares the same path.
    #[test]
    fn on_type_change_keeps_shared_path_still_owned_by_another_agent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let shared = tmp.path().to_path_buf();

        std::fs::write(shared.join("co-owned.yaml"), "data").unwrap();
        std::fs::write(shared.join("exclusive.yaml"), "data").unwrap();

        let old_type_id = AgentTypeID::try_from("test/oldagent:0.0.1").unwrap();
        let new_type_id = AgentTypeID::try_from("test/newagent:0.0.2").unwrap();
        let other_type_id = AgentTypeID::try_from("test/otheragent:0.0.1").unwrap();

        // Old type declares both files; new type declares neither.
        let old_definition = host_type_with_shared(serde_json::json!({
            "co-owned.yaml":  { "kind": "file", "text": "data" },
            "exclusive.yaml": { "kind": "file", "text": "data" },
        }));
        let new_definition = host_type_with_shared(serde_json::json!({}));
        // The other active agent independently declares the co-owned file.
        let other_definition = host_type_with_shared(serde_json::json!({
            "co-owned.yaml": { "kind": "file", "text": "data" },
        }));

        let mut registry = MockAgentTypeRegistry::new();
        // Two lookups per type-bumped agent type (agent fs + shared); one for the other agent.
        registry.should_get(old_type_id.clone(), &old_definition);
        registry.should_get(old_type_id.clone(), &old_definition);
        registry.should_get(new_type_id.clone(), &new_definition);
        registry.should_get(new_type_id.clone(), &new_definition);
        registry.should_get(other_type_id.clone(), &other_definition);

        let active_agents = configured(&[
            ("test-agent", "test/newagent:0.0.2"),
            ("other-agent", "test/otheragent:0.0.1"),
        ]);

        shared_cleaner(registry, shared.clone())
            .reconcile_filesystems_on_type_change(
                &agent_id("test-agent"),
                &old_type_id,
                &new_type_id,
                &active_agents,
            )
            .expect("reconcile must succeed");

        assert!(
            shared.join("co-owned.yaml").exists(),
            "path still declared by another active agent must not be deleted"
        );
        assert!(
            !shared.join("exclusive.yaml").exists(),
            "path not declared by any active agent must be deleted"
        );
    }
}
