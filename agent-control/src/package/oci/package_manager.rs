use std::{
    io,
    path::{Component, Path, PathBuf},
};

use fs::{
    LocalFile,
    directory_manager::{DirectoryManagementError, DirectoryManager, DirectoryManagerFs},
    file_deleter::FileDeleter,
    file_renamer::{FileRenamer, FileRenamerError},
};
use oci_client::Reference;
use thiserror::Error;
use tracing::{debug, warn};

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
    FR: FileRenamer + FileDeleter,
{
    pub downloader: D,
    pub directory_manager: DM,
    pub file_manager: FR,
    pub base_path: PathBuf, // this would be the `auto-generated` directory
}

#[derive(Debug, Error)]
pub enum OCIPackageManagerError {
    #[error("error attempting to download OCI artifact: {0}")]
    Download(OCIDownloaderError),
    #[error("error attempting to install OCI artifact: {0}")]
    Install(io::Error),
    #[error("error attempting to uninstall OCI artifact: {0}")]
    Uninstall(io::Error),
    // The below variants should be removed when the `fs` traits are refactored and they return
    // `std::io::Error`s instead.
    #[error("directory management error: {0}")]
    Directory(DirectoryManagementError),
    #[error("file rename error: {0}")]
    Rename(FileRenamerError),
    // Naming produces a non-normalized suffix. Should not happen but we can identify bugs with it.
    #[error("Package reference naming validation produces a non-normalized suffix: {0}")]
    NotNormalSuffix(String),
}

const DOWNLOADED_PACKAGES_LOCATION: &str = "__temp_packages";
const INSTALLED_PACKAGES_LOCATION: &str = "packages";

impl<D, DM, FR> OCIPackageManager<D, DM, FR>
where
    D: OCIDownloader,
    DM: DirectoryManager,
    FR: FileRenamer + FileDeleter,
{
    /// Validates that the provided vector of paths contains exactly one path (i.e. a single file)
    /// was downloaded from the [`OCIDownloader`]) and retrieve its [`PathBuf`], otherwise fail.
    fn try_get_unique_path(paths: Vec<PathBuf>) -> Result<PathBuf, OCIPackageManagerError> {
        if paths.len() != 1 {
            let paths_len = paths.len();
            Err(OCIPackageManagerError::Install(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected a single file in the OCI artifact, found {paths_len} files",),
            )))
        } else {
            Ok(paths
                .into_iter()
                .next()
                .expect("checked vector for length above >= 1"))
        }
    }

    /// Moves the downloaded package file from `download_filepath` to its final install location.
    ///
    /// This final location is determined from the package [`Reference`]. If the move fails the
    /// destination directory is deleted.
    fn install_package(
        &self,
        agent_id: &AgentID,
        downloaded_filepath: &Path,
        artifact_name: PathBuf,
    ) -> Result<PathBuf, OCIPackageManagerError> {
        // Build and create destination directory
        let final_file_dir = self
            .base_path
            .join(agent_id)
            .join(INSTALLED_PACKAGES_LOCATION);
        self.directory_manager
            .create(&final_file_dir)
            .map_err(OCIPackageManagerError::Directory)?;
        let install_path = final_file_dir.join(&artifact_name);
        // The "install" action itself. Moves the downloaded file to its final location.
        self.file_manager
            .rename(downloaded_filepath, &install_path)
            .map_err(|e| {
                warn!("Package installation failed: {e}");
                OCIPackageManagerError::Rename(e)
            })?;

        debug!(
            "Package installation succeeded. Written to {}",
            install_path.display()
        );
        Ok(install_path)
    }
}

/// Computes the download destination of a package [`Reference`] depending on the available fields.
///
/// The path is computed by sanitizing the package reference string to ensure it is a valid filename
/// on both Windows and Unix, and to prevent path traversal or injection.
///
/// The sanitization process:
/// 1. Prepends "oci_" to the filename to avoid reserved filenames (e.g. "CON" on Windows) and
///    to prevent the filename from being exactly "." or "..".
/// 2. Replaces directory separators (`/`, `\`) and the tag separator (`:`) with `__`.
/// 3. Replaces any other character that is not alphanumeric, `.`, `-`, `_`, or `@` with `_`.
pub fn compute_path_suffix(package: &Reference) -> Result<PathBuf, OCIPackageManagerError> {
    let package_full_reference = package.whole();
    let mut safe_name = String::with_capacity(package_full_reference.len() + 4);
    safe_name.push_str("oci_");
    for c in package_full_reference.chars() {
        match c {
            c if std::path::is_separator(c) => safe_name.push_str("__"),
            c if !c.is_alphanumeric() => safe_name.push('_'),
            c => safe_name.push(c),
        }
    }

    let sanitized_path = PathBuf::from(safe_name);

    // Make sure this doesn't have any non-normal component (root ref, parent dir ref, etc)
    sanitized_path.components().try_for_each(|c| match c {
        Component::Normal(_) => Ok(()),
        other => Err(OCIPackageManagerError::NotNormalSuffix(format!(
            "{other:?}"
        ))),
    })?;

    Ok(sanitized_path)
}

impl<D, DM, FR> PackageManager for OCIPackageManager<D, DM, FR>
where
    D: OCIDownloader,
    DM: DirectoryManager,
    FR: FileRenamer + FileDeleter,
{
    /// Installs the given OCI package for the specified agent.
    ///
    /// This method downloads the package to a temporary location and then moves it to its final
    /// installation directory. The final location is determined based on the package reference.
    ///
    /// The temporary location is deleted before this function returns, regardless of the install
    /// success or failure.
    fn install(
        &self,
        agent_id: &AgentID,
        package: &Reference,
    ) -> Result<PathBuf, OCIPackageManagerError> {
        // Using the whole reference (including tag/digest if available) with special chars replaces as the download path suffix (see function doc for details)
        let path_suffix = compute_path_suffix(package)?;

        let temp_download_dir = self
            .base_path
            .join(agent_id)
            .join(DOWNLOADED_PACKAGES_LOCATION)
            .join(&path_suffix);

        let download_dir_creation_result = self
            .directory_manager
            .create(&temp_download_dir)
            .map_err(OCIPackageManagerError::Directory);

        let downloaded_pkg = download_dir_creation_result
            .and_then(|_| {
                self.downloader
                    .download(package, &temp_download_dir)
                    .map_err(OCIPackageManagerError::Download)
            })
            .and_then(Self::try_get_unique_path);

        let installed_package = downloaded_pkg
            .and_then(|filepath| self.install_package(agent_id, &filepath, path_suffix))
            .inspect(|p| debug!("OCI package installed at {}", p.display()))
            .inspect_err(|e| warn!("OCI package installation failed: {}", e));

        // Delete temporary download directory after use regardless of success or failure
        // (this is why I'm not using `?` above!)
        self.directory_manager
            .delete(&temp_download_dir)
            .map_err(OCIPackageManagerError::Directory)
            // Everything went fine. Return the installed package result
            .and(installed_package)
    }

    fn uninstall(&self, _agent_id: &AgentID, package: &Path) -> Result<(), OCIPackageManagerError> {
        self.file_manager
            .delete(package)
            .map_err(OCIPackageManagerError::Uninstall)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::oci::downloader::tests::MockOCIDownloader;
    use fs::directory_manager::mock::MockDirectoryManager;
    use fs::mock::MockLocalFile;
    use mockall::Sequence;
    use mockall::predicate::eq;
    use oci_spec::distribution::Reference;
    use std::str::FromStr;

    #[test]
    fn test_install_success() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let mut file_manager = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        // This test does not perform any I/O, but needs a valid reference to build the value
        let reference =
            Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
                .unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir
            .join(DOWNLOADED_PACKAGES_LOCATION)
            .join(compute_path_suffix(&reference).unwrap());
        let downloaded_file = download_dir.join("layer_digest.tar.gz");
        let install_dir = root_dir.join(INSTALLED_PACKAGES_LOCATION);
        let install_path = install_dir.join(compute_path_suffix(&reference).unwrap());

        let mut dir_manipulation_sequence = Sequence::new();
        directory_manager
            .expect_create()
            .with(eq(download_dir.clone()))
            .in_sequence(&mut dir_manipulation_sequence)
            .once()
            .returning(|_| Ok(()));

        directory_manager
            .expect_create()
            .with(eq(install_dir.clone()))
            .in_sequence(&mut dir_manipulation_sequence)
            .once()
            .returning(|_| Ok(()));

        directory_manager
            .expect_delete()
            .with(eq(download_dir.clone()))
            .in_sequence(&mut dir_manipulation_sequence)
            .once()
            .returning(|_| Ok(()));

        downloader
            .expect_download()
            .with(eq(reference.clone()), eq(download_dir.clone()))
            .once()
            .returning(move |_, _| Ok(vec![downloaded_file.clone()]));

        file_manager
            .expect_rename()
            .with(
                eq(download_dir.join("layer_digest.tar.gz")),
                eq(install_path.clone()),
            )
            .once()
            .returning(|_, _| Ok(()));

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            file_manager,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, &reference);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), install_path);
    }

    #[test]
    fn test_install_directory_creation_failure() {
        let downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let file_manager = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir
            .join(DOWNLOADED_PACKAGES_LOCATION)
            .join(compute_path_suffix(&reference).unwrap());

        directory_manager
            .expect_create()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| {
                Err(DirectoryManagementError::ErrorCreatingDirectory(
                    "path".into(),
                    "error".into(),
                ))
            });

        directory_manager
            .expect_delete()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            file_manager,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, &reference);

        assert!(matches!(result, Err(OCIPackageManagerError::Directory(_))));
    }

    #[test]
    fn test_install_download_failure() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let file_manager = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir
            .join(DOWNLOADED_PACKAGES_LOCATION)
            .join(compute_path_suffix(&reference).unwrap());

        directory_manager
            .expect_create()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        directory_manager
            .expect_delete()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        downloader
            .expect_download()
            .with(eq(reference.clone()), eq(download_dir))
            .once()
            .returning(|_, _| {
                Err(OCIDownloaderError::DownloadingArtifact(
                    "download failed".into(),
                ))
            });

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            file_manager,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, &reference);

        assert!(matches!(result, Err(OCIPackageManagerError::Download(_))));
    }

    #[test]
    fn test_install_invalid_download_no_files() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let file_manager = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir
            .join(DOWNLOADED_PACKAGES_LOCATION)
            .join(compute_path_suffix(&reference).unwrap());

        directory_manager
            .expect_create()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        directory_manager
            .expect_delete()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        downloader
            .expect_download()
            .with(eq(reference.clone()), eq(download_dir))
            .once()
            .returning(|_, _| Ok(vec![])); // Empty vector

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            file_manager,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, &reference);

        assert!(matches!(
            result,
            Err(OCIPackageManagerError::Install(e)) if e.kind() == io::ErrorKind::InvalidData
        ));
    }

    #[test]
    fn test_install_invalid_download_multiple_files() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let file_manager = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir
            .join(DOWNLOADED_PACKAGES_LOCATION)
            .join(compute_path_suffix(&reference).unwrap());

        directory_manager
            .expect_create()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        directory_manager
            .expect_delete()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        downloader
            .expect_download()
            .with(eq(reference.clone()), eq(download_dir))
            .once()
            .returning(|_, _| Ok(vec![PathBuf::from("file1"), PathBuf::from("file2")]));

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            file_manager,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, &reference);

        assert!(matches!(
            result,
            Err(OCIPackageManagerError::Install(e)) if e.kind() == io::ErrorKind::InvalidData
        ));
    }

    #[test]
    fn test_install_rename_failure() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let mut file_manager = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir
            .join(DOWNLOADED_PACKAGES_LOCATION)
            .join(compute_path_suffix(&reference).unwrap());
        let downloaded_file = download_dir.join("layer_digest.tar.gz");
        let install_dir = root_dir.join(INSTALLED_PACKAGES_LOCATION);
        let install_path = install_dir.join(compute_path_suffix(&reference).unwrap());

        directory_manager
            .expect_create()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        directory_manager
            .expect_create()
            .with(eq(install_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        directory_manager
            .expect_delete()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        downloader
            .expect_download()
            .with(eq(reference.clone()), eq(download_dir.clone()))
            .once()
            .returning(move |_, _| Ok(vec![downloaded_file.clone()]));

        file_manager
            .expect_rename()
            .with(
                eq(download_dir.join("layer_digest.tar.gz")),
                eq(install_path.clone()),
            )
            .once()
            .returning(|_, _| {
                Err(FileRenamerError::Rename(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "denied",
                )))
            });

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            file_manager,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, &reference);

        assert!(matches!(result, Err(OCIPackageManagerError::Rename(_))));
    }

    #[test]
    fn test_install_final_directory_creation_failure() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let file_manager = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir
            .join(DOWNLOADED_PACKAGES_LOCATION)
            .join(compute_path_suffix(&reference).unwrap());
        let downloaded_file = download_dir.join("layer_digest.tar.gz");
        let install_dir = root_dir.join(INSTALLED_PACKAGES_LOCATION);

        directory_manager
            .expect_create()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        downloader
            .expect_download()
            .with(eq(reference.clone()), eq(download_dir.clone()))
            .once()
            .returning(move |_, _| Ok(vec![downloaded_file.clone()]));

        directory_manager
            .expect_create()
            .with(eq(install_dir.clone()))
            .once()
            .returning(|_| {
                Err(DirectoryManagementError::ErrorCreatingDirectory(
                    "path".into(),
                    "error".into(),
                ))
            });

        directory_manager
            .expect_delete()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            file_manager,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, &reference);

        assert!(matches!(result, Err(OCIPackageManagerError::Directory(_))));
    }

    #[test]
    fn test_install_cleanup_failure() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let mut file_manager = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir
            .join(DOWNLOADED_PACKAGES_LOCATION)
            .join(compute_path_suffix(&reference).unwrap());
        let downloaded_file = download_dir.join("layer_digest.tar.gz");
        let install_dir = root_dir.join(INSTALLED_PACKAGES_LOCATION);
        let install_path = install_dir.join(compute_path_suffix(&reference).unwrap());

        directory_manager
            .expect_create()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        directory_manager
            .expect_create()
            .with(eq(install_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        downloader
            .expect_download()
            .with(eq(reference.clone()), eq(download_dir.clone()))
            .once()
            .returning(move |_, _| Ok(vec![downloaded_file.clone()]));

        file_manager
            .expect_rename()
            .with(
                eq(download_dir.join("layer_digest.tar.gz")),
                eq(install_path.clone()),
            )
            .once()
            .returning(|_, _| Ok(()));

        directory_manager
            .expect_delete()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| {
                Err(DirectoryManagementError::ErrorDeletingDirectory(
                    "some error".into(),
                ))
            });

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            file_manager,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, &reference);

        assert!(matches!(result, Err(OCIPackageManagerError::Directory(_))));
    }

    #[test]
    fn test_uninstall_success() {
        let downloader = MockOCIDownloader::new();
        let directory_manager = MockDirectoryManager::new();
        let mut file_manager = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let package_path = PathBuf::from("/path/to/package");

        file_manager
            .expect_delete()
            .with(eq(package_path.clone()))
            .once()
            .returning(|_| Ok(()));

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            file_manager,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.uninstall(&agent_id, &package_path);

        assert!(result.is_ok());
    }

    #[test]
    fn test_uninstall_failure() {
        let downloader = MockOCIDownloader::new();
        let directory_manager = MockDirectoryManager::new();
        let mut file_manager = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let package_path = PathBuf::from("/path/to/package");

        file_manager
            .expect_delete()
            .with(eq(package_path.clone()))
            .once()
            .returning(|_| Err(io::Error::new(io::ErrorKind::NotFound, "not found")));

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            file_manager,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.uninstall(&agent_id, &package_path);

        assert!(matches!(result, Err(OCIPackageManagerError::Uninstall(_))));
    }
}
