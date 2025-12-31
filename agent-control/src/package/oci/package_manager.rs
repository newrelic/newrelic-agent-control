use std::{
    io::{Error as IoError, ErrorKind},
    path::PathBuf,
};

use oci_client::Reference;
use thiserror::Error;

use crate::{agent_control::agent_id::AgentID, package::manager::PackageManager};

use super::downloader::{OCIDownloader, OCIDownloaderError};

pub struct OCIPackageManager {
    pub pkg_downloader: OCIDownloader,
    pub base_path: PathBuf, // this would be the `auto-generated` directory
}

#[derive(Debug, Error)]
pub enum OCIPackageManagerError {
    #[error("error attempting to download OCI artifact: {0}")]
    Download(OCIDownloaderError),
    #[error("error attempting to install OCI artifact: {0}")]
    Install(IoError),
    #[error("error attempting to uninstall OCI artifact: {0}")]
    Uninstall(IoError),
}

impl PackageManager for OCIPackageManager {
    type Error = OCIPackageManagerError;
    type Package = Reference;
    type InstalledPackage = PathBuf; // Downloaded package location

    fn install(
        &self,
        agent_id: &AgentID,
        package: Self::Package,
    ) -> Result<Self::InstalledPackage, Self::Error> {
        let install_path = self.base_path.join(agent_id).join("__packages");

        let downloaded_paths = self
            .pkg_downloader
            .download_artifact(&package, &install_path)
            .map_err(OCIPackageManagerError::Download)?;

        // do something with the downloaded file
        // validations should be applied
        // in particular, I am assuming that the OCI artifact downloaded consists of a single file,
        // this file should be renamed to the name of the repository and moved to the install_path
        let unique_path = validate_single_path(downloaded_paths)?;
        let digest = package.digest().ok_or_else(|| {
            OCIPackageManagerError::Install(IoError::new(
                ErrorKind::InvalidData,
                "OCI reference missing digest".to_string(),
            ))
        })?;
        let repo_name = package.repository();
        let downloaded_file_path = install_path.join(repo_name);

        Ok(install_path)
    }

    fn uninstall(
        &self,
        _agent_id: &AgentID,
        _package: Self::InstalledPackage,
    ) -> Result<(), Self::Error> {
        todo!("uninstall not implemented yet")
    }
}

/// Validates that the provided vector of paths contains exactly one path (i.e., a single downloaded file).
/// Returns the single path if validation passes, otherwise returns an error.
fn validate_single_path(paths: Vec<PathBuf>) -> Result<PathBuf, OCIPackageManagerError> {
    if paths.len() != 1 {
        let paths_len = paths.len();
        Err(OCIPackageManagerError::Install(IoError::new(
            ErrorKind::InvalidData,
            format!("expected a single file in the OCI artifact, found {paths_len} files",),
        )))
    } else {
        Ok(paths
            .into_iter()
            .next()
            .expect("checked vector for length above >= 1"))
    }
}
