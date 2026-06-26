//! On-host deployment configuration after templating.
use crate::agent_type::runtime_config::on_host::package::PackageID;
use crate::agent_type::runtime_config::on_host::package::rendered::Package;
use crate::agent_type::runtime_config::{
    health_config::rendered::OnHostHealthConfig,
    on_host::{executable::rendered::Executable, filesystem::rendered::FileSystem},
};
use std::collections::HashMap;

/// On-host deployment configuration after templating.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct OnHost {
    /// The executables to supervise.
    pub executables: Vec<Executable>,
    /// Whether file logging is enabled.
    pub enable_file_logging: bool,
    /// Enables and define health checks configuration.
    pub health: OnHostHealthConfig,
    /// Files and directories to materialize on disk.
    pub filesystem: FileSystem,
    /// Packages to download for this agent.
    pub packages: RenderedPackages,
}

/// Rendered packages keyed by their [`PackageID`].
pub type RenderedPackages = HashMap<PackageID, Package>;
