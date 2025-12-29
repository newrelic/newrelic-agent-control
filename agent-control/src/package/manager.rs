//! This module manages package operations such as installation, removal, and updates.

use crate::agent_control::agent_id::AgentID;

/// An interface for a package manager.
///
/// This trait has associated types for the error, the package to install and the installed package.
///
/// Given the intended usage for this trait is host-based, implementations will likely rely on
/// filesystem interaction.
pub trait PackageManager {
    /// Errors that may occur
    type Error: std::error::Error;
    /// The package to be installed.
    /// It should contain all necessary information for a successful installation.
    type Package;
    /// The package after it has been installed.
    /// It should contain all relevant information about the installed package
    /// so it can be managed or queried later.
    type InstalledPackage;

    /// Install a package.
    fn install(
        &self,
        agent_id: &AgentID,
        package: Self::Package,
    ) -> Result<Self::InstalledPackage, Self::Error>;

    /// Uninstall a package.
    fn uninstall(
        &self,
        agent_id: &AgentID,
        package: Self::InstalledPackage,
    ) -> Result<(), Self::Error>;
}
