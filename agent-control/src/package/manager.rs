//! This module manages package operations such as installation, removal, and updates.

use std::path::PathBuf;

use oci_client::Reference;

use crate::{
    agent_control::agent_id::AgentID, package::oci::package_manager::OCIPackageManagerError,
};

/// Information required to reference and install a package
#[derive(Debug)]
pub struct PackageData {
    pub id: String, // same type as the packages map on an agent type definition
    pub oci_reference: Reference,
}

/// Information about an installed package
#[derive(Debug)]
pub struct InstalledPackageData {
    pub id: String, // same type as the packages map on an agent type definition
    pub installation_path: PathBuf,
}

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
        package: PackageData,
    ) -> Result<InstalledPackageData, OCIPackageManagerError>;

    /// Uninstall a package.
    fn uninstall(
        &self,
        agent_id: &AgentID,
        package: InstalledPackageData,
    ) -> Result<(), OCIPackageManagerError>;
}
