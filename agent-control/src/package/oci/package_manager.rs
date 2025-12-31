use std::{
    io::{Error as IoError, ErrorKind},
    path::PathBuf,
};

use fs::{
    LocalFile,
    directory_manager::{DirectoryManagementError, DirectoryManager, DirectoryManagerFs},
    file_renamer::{FileRenamer, FileRenamerError},
};
use oci_client::Reference;
use thiserror::Error;

use crate::{
    agent_control::agent_id::AgentID,
    package::{manager::PackageManager, oci::downloader::OCIRefDownloader},
};

use super::downloader::{OCIDownloader, OCIDownloaderError};

pub type DefaultOCIPackageManager =
    OCIPackageManager<OCIRefDownloader, DirectoryManagerFs, LocalFile>;

pub struct OCIPackageManager<D, DM, FR>
where
    D: OCIDownloader,
    DM: DirectoryManager,
    FR: FileRenamer,
{
    pub pkg_downloader: D,
    pub directory_manager: DM,
    pub file_renamer: FR,
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
    #[error("directory management error: {0}")]
    Directory(#[from] DirectoryManagementError),
    #[error("file rename error: {0}")]
    Rename(#[from] FileRenamerError),
}

impl<D, DM, FR> PackageManager for OCIPackageManager<D, DM, FR>
where
    D: OCIDownloader,
    DM: DirectoryManager,
    FR: FileRenamer,
{
    type Error = OCIPackageManagerError;
    type Package = Reference;
    type InstalledPackage = PathBuf; // Downloaded package location

    fn install(
        &self,
        agent_id: &AgentID,
        package: Self::Package,
    ) -> Result<Self::InstalledPackage, Self::Error> {
        // Package will:
        //   1. Download into `<BASE_PATH>/<AGENT_ID>/packages/<LAYER_DIGEST>`
        //   2. Be moved to `<BASE_PATH>/<AGENT_ID>/packages/<REPOSITORY>_<TAG>`
        // Where `<BASE_PATH>` is by default AC's auto-generated directory.
        let digest = package.digest().ok_or_else(|| {
            OCIPackageManagerError::Install(IoError::new(
                ErrorKind::InvalidData,
                "OCI reference missing digest".to_string(),
            ))
        })?;

        let install_dir = self.base_path.join(agent_id).join("packages").join(digest);

        // 1. Ensure the directory exists
        self.directory_manager.create(&install_dir)?;

        // 2. Actually download the package. The implementation of the downloader saves files
        // using the layer digest as the filename.
        let downloaded_paths = self
            .pkg_downloader
            .download(&package, &install_dir)
            .map_err(OCIPackageManagerError::Download)?;

        // 3. Validate we have exactly one file and retrieve its path
        let unique_temp_file_path = validate_single_path(downloaded_paths)?;

        // 4. Rename the file to match the schema `<REPOSITORY>_<TAG>`
        let repo = package.repository();
        let tag = package.tag().unwrap_or("latest");
        let file_name = format!("{repo}_{tag}").replace("/", "_");
        let final_file_path = install_dir.join(file_name);

        self.file_renamer
            .rename(&unique_temp_file_path, &final_file_path)?;

        Ok(final_file_path)
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
