//! This module manages package operations such as installation, removal, and updates.
use crate::agent_control::agent_id::AgentID;
use crate::agent_type::runtime_config::on_host::package::rendered::{Oci, PostDownloadHook};
use crate::package::oci::package_manager::OCIPackageManagerError;
use std::path::PathBuf;

/// Information required to reference and install a package
#[derive(Debug, Clone, PartialEq)]
pub struct PackageData {
    /// Package identifier (same type as the packages map on an agent type definition).
    pub id: String, // same type as the packages map on an agent type definition
    /// OCI reference describing where to fetch the package from.
    pub oci: Oci,
    /// Optional hook to run after the package has been downloaded.
    pub post_download_hook: Option<PostDownloadHook>,
}

/// Information about an installed package
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledPackageData {
    /// Package identifier (same type as the packages map on an agent type definition).
    pub id: String, // same type as the packages map on an agent type definition
    /// Filesystem path where the package was installed.
    pub installation_path: PathBuf,
}

/// An interface for a package manager.
///
/// This trait has associated types for the error, the package to install and the installed package.
///
/// Given the intended usage for this trait is host-based, implementations will likely rely on
/// filesystem interaction.
pub trait PackageManager: Send + Sync {
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

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use super::*;
    use mockall::mock;
    use std::sync::Arc;

    mock! {
        pub PackageManager {}
        impl PackageManager for PackageManager {
            fn install(
                &self,
                agent_id: &AgentID,
                package: PackageData,
            ) -> Result<InstalledPackageData, OCIPackageManagerError>;
            fn uninstall(
                &self,
                agent_id: &AgentID,
                package: InstalledPackageData,
            ) -> Result<(), OCIPackageManagerError>;
        }
    }

    impl MockPackageManager {
        pub fn new_arc() -> Arc<Self> {
            Arc::new(MockPackageManager::new())
        }
    }
}
