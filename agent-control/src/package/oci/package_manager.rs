use std::path::PathBuf;

use oci_client::Reference;

use crate::{agent_control::agent_id::AgentID, package::manager::PackageManager};

use super::downloader::{OCIDownloader, OCIDownloaderError};

pub struct OCIPackageManager {
    pub pkg_downloader: OCIDownloader,
    pub base_path: PathBuf, // this would be the `auto-generated` directory
}

impl PackageManager for OCIPackageManager {
    type Error = OCIDownloaderError;

    type Package = Reference;

    type InstalledPackage = PathBuf; // Downloaded package location

    fn install(
        &self,
        agent_id: &AgentID,
        package: Self::Package,
    ) -> Result<Self::InstalledPackage, Self::Error> {
        let install_path = self.base_path.join(agent_id);
        std::fs::create_dir_all(&install_path).map_err(|err| {
            OCIDownloaderError::DownloadingArtifactError(format!(
                "Failed to create package directory: {}",
                err
            ))
        })?;

        self.pkg_downloader
            .download_artifact(&package, &install_path)?;

        Ok(install_path)
    }

    fn uninstall(
        &self,
        _agent_id: &AgentID,
        package: Self::InstalledPackage,
    ) -> Result<(), Self::Error> {
        if package.exists() {
            std::fs::remove_dir_all(&package).map_err(|err| {
                OCIDownloaderError::DownloadingArtifactError(format!(
                    "Failed to remove package directory: {}",
                    err
                ))
            })?;
        }
        Ok(())
    }
}
