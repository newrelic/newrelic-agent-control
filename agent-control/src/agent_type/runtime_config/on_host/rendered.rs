//! On-host deployment configuration after templating.
use crate::agent_type::runtime_config::on_host::package::PackageID;
use crate::agent_type::runtime_config::on_host::package::rendered::Package;
use crate::agent_type::runtime_config::{
    health_config::rendered::OnHostHealthConfig,
    on_host::{
        executable::rendered::Executable,
        filesystem::rendered::{FileSystem, SharedFileSystem},
    },
};
use std::collections::HashMap;

/// On-host deployment configuration after templating.
#[derive(Debug, Clone, PartialEq)]
pub struct OnHost {
    /// The executables to supervise.
    pub executables: Vec<Executable>,
    /// Whether file logging is enabled.
    pub enable_file_logging: bool,
    /// Enables and define health checks configuration.
    pub health: Option<OnHostHealthConfig>,
    /// Files and directories to materialize on disk for each agent.
    pub filesystem: FileSystem,
    /// Files and directories to materialize in the base shared across sub-agents.
    pub shared_filesystem: SharedFileSystem,
    /// Packages to download for this agent.
    pub packages: RenderedPackages,
    /// Main OCI Package version reported as the `agent.version`.
    pub reported_version_package: Option<PackageID>,
}

/// Rendered packages keyed by their [`PackageID`].
pub type RenderedPackages = HashMap<PackageID, Package>;
