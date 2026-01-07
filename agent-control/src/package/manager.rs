//! This module manages package operations such as installation, removal, and updates.

use std::path::{Path, PathBuf};

use oci_client::Reference;

use crate::{
    agent_control::agent_id::AgentID, package::oci::package_manager::OCIPackageManagerError,
};

/// An interface for a package manager.
///
/// This trait has associated types for the error, the package to install and the installed package.
///
/// Given the intended usage for this trait is host-based, implementations will likely rely on
/// filesystem interaction.
pub trait PackageManager {
    /// Install a package.
    fn install(
        &self,
        agent_id: &AgentID,
        package: &Reference,
    ) -> Result<PathBuf, OCIPackageManagerError>;

    /// Uninstall a package.
    fn uninstall(&self, agent_id: &AgentID, package: &Path) -> Result<(), OCIPackageManagerError>;
}
