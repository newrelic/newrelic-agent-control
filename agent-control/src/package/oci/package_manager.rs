use std::{
    io::{Error as IoError, ErrorKind},
    path::{Path, PathBuf},
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
    // The below variants should be removed when the `fs` traits are refactored and they return
    // `std::io::Error`s instead.
    #[error("directory management error: {0}")]
    Directory(DirectoryManagementError),
    #[error("file rename error: {0}")]
    Rename(FileRenamerError),
}

const DOWNLOADED_PACKAGES_LOCATION: &str = "__temp_packages";
const INSTALLED_PACKAGES_LOCATION: &str = "packages";

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

        let temp_download_dir = self
            .base_path
            .join(agent_id)
            .join(DOWNLOADED_PACKAGES_LOCATION)
            .join(digest);

        // 1. Ensure the directory exists
        // TODO PR review: should we actually need this or delegate this to the downloader impl?
        self.directory_manager
            .create(&temp_download_dir)
            .map_err(OCIPackageManagerError::Directory)?;

        // 2. Download and move the package
        let installation_result =
            self.download_and_move_package(agent_id, &package, &temp_download_dir);

        // Delete temporary download directory after use
        self.directory_manager
            .delete(&temp_download_dir)
            .map_err(OCIPackageManagerError::Directory)?;

        installation_result
    }

    fn uninstall(
        &self,
        _agent_id: &AgentID,
        _package: Self::InstalledPackage,
    ) -> Result<(), Self::Error> {
        todo!("uninstall not implemented yet")
    }
}

impl<D, DM, FR> OCIPackageManager<D, DM, FR>
where
    D: OCIDownloader,
    DM: DirectoryManager,
    FR: FileRenamer,
{
    fn download_and_move_package(
        &self,
        agent_id: &AgentID,
        package: &Reference,
        temp_install_dir: &Path,
    ) -> Result<PathBuf, OCIPackageManagerError> {
        // 2. Actually download the package. The implementation of the downloader saves files
        // using the layer digest as the filename.
        let downloaded_paths = self
            .pkg_downloader
            .download(package, temp_install_dir)
            .map_err(OCIPackageManagerError::Download)?;

        // 3. Validate we have exactly one file and retrieve its path
        let unique_temp_file_path = Self::validate_single_path(downloaded_paths)?;

        // 4. Rename the file to match the schema `<REPOSITORY>_<TAG>`
        let repo = package.repository();
        let tag = package.tag().unwrap_or("latest");
        let file_name = format!("{repo}_{tag}").replace("/", "_");
        let final_file_dir = self
            .base_path
            .join(agent_id)
            .join(INSTALLED_PACKAGES_LOCATION);

        // Ensure final dir path exists
        self.directory_manager
            .create(&final_file_dir)
            .map_err(OCIPackageManagerError::Directory)?;

        let final_file_path = final_file_dir.join(file_name);

        match self
            .file_renamer
            .rename(&unique_temp_file_path, &final_file_path)
        {
            // On success, return the path of the installed package so it can be used elsewhere
            // (locating the binary for running, uninstalling, etc)
            Ok(()) => Ok(final_file_path),
            // On failure, remove installation path due to failed operation and propagate error
            Err(e) => {
                self.directory_manager
                    .delete(&final_file_dir)
                    .map_err(OCIPackageManagerError::Directory)?;
                Err(OCIPackageManagerError::Rename(e))
            }
        }
    }

    /// Validates that the provided vector of paths contains exactly one path (i.e. a single downloaded file).
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
        let mut file_renamer = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let digest = reference.digest().unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir.join(DOWNLOADED_PACKAGES_LOCATION).join(digest);
        let downloaded_file = download_dir.join("layer_digest.tar.gz");
        let install_dir = root_dir.join(INSTALLED_PACKAGES_LOCATION);
        let install_path = install_dir.join("library_busybox_latest");

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

        file_renamer
            .expect_rename()
            .with(
                eq(download_dir.join("layer_digest.tar.gz")),
                eq(install_path.clone()),
            )
            .once()
            .returning(|_, _| Ok(()));

        let pm = OCIPackageManager {
            pkg_downloader: downloader,
            directory_manager,
            file_renamer,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, reference);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), install_path);
    }

    #[test]
    fn test_install_missing_digest() {
        let downloader = MockOCIDownloader::new();
        let directory_manager = MockDirectoryManager::new();
        let file_renamer = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest").unwrap(); // No digest

        let pm = OCIPackageManager {
            pkg_downloader: downloader,
            directory_manager,
            file_renamer,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, reference);

        assert!(matches!(
            result,
            Err(OCIPackageManagerError::Install(e)) if e.kind() == ErrorKind::InvalidData
        ));
    }

    #[test]
    fn test_install_directory_creation_failure() {
        let downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let file_renamer = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let digest = reference.digest().unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir.join(DOWNLOADED_PACKAGES_LOCATION).join(digest);

        directory_manager
            .expect_create()
            .with(eq(download_dir))
            .once()
            .returning(|_| {
                Err(DirectoryManagementError::ErrorCreatingDirectory(
                    "path".into(),
                    "error".into(),
                ))
            });

        let pm = OCIPackageManager {
            pkg_downloader: downloader,
            directory_manager,
            file_renamer,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, reference);

        assert!(matches!(result, Err(OCIPackageManagerError::Directory(_))));
    }

    #[test]
    fn test_install_download_failure() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let file_renamer = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let digest = reference.digest().unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir.join(DOWNLOADED_PACKAGES_LOCATION).join(digest);

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
            pkg_downloader: downloader,
            directory_manager,
            file_renamer,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, reference);

        assert!(matches!(result, Err(OCIPackageManagerError::Download(_))));
    }

    #[test]
    fn test_install_invalid_download_no_files() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let file_renamer = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let digest = reference.digest().unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir.join(DOWNLOADED_PACKAGES_LOCATION).join(digest);

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
            pkg_downloader: downloader,
            directory_manager,
            file_renamer,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, reference);

        assert!(matches!(
            result,
            Err(OCIPackageManagerError::Install(e)) if e.kind() == ErrorKind::InvalidData
        ));
    }

    #[test]
    fn test_install_invalid_download_multiple_files() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let file_renamer = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let digest = reference.digest().unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir.join(DOWNLOADED_PACKAGES_LOCATION).join(digest);

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
            pkg_downloader: downloader,
            directory_manager,
            file_renamer,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, reference);

        assert!(matches!(
            result,
            Err(OCIPackageManagerError::Install(e)) if e.kind() == ErrorKind::InvalidData
        ));
    }

    #[test]
    fn test_install_rename_failure() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let mut file_renamer = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let digest = reference.digest().unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir.join(DOWNLOADED_PACKAGES_LOCATION).join(digest);
        let downloaded_file = download_dir.join("layer_digest.tar.gz");
        let install_dir = root_dir.join(INSTALLED_PACKAGES_LOCATION);
        let install_path = install_dir.join("library_busybox_latest");

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

        file_renamer
            .expect_rename()
            .with(
                eq(download_dir.join("layer_digest.tar.gz")),
                eq(install_path.clone()),
            )
            .once()
            .returning(|_, _| {
                Err(FileRenamerError::Rename(IoError::new(
                    ErrorKind::PermissionDenied,
                    "denied",
                )))
            });

        let pm = OCIPackageManager {
            pkg_downloader: downloader,
            directory_manager,
            file_renamer,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, reference);

        assert!(matches!(result, Err(OCIPackageManagerError::Rename(_))));
    }

    #[test]
    fn test_install_final_directory_creation_failure() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let file_renamer = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let digest = reference.digest().unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir.join(DOWNLOADED_PACKAGES_LOCATION).join(digest);
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
            pkg_downloader: downloader,
            directory_manager,
            file_renamer,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, reference);

        assert!(matches!(result, Err(OCIPackageManagerError::Directory(_))));
    }

    #[test]
    fn test_install_cleanup_failure() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();
        let mut file_renamer = MockLocalFile::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let reference = Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap();
        let digest = reference.digest().unwrap();
        let root_dir = PathBuf::from("/tmp/base/agent-id");
        let download_dir = root_dir.join(DOWNLOADED_PACKAGES_LOCATION).join(digest);
        let downloaded_file = download_dir.join("layer_digest.tar.gz");
        let install_dir = root_dir.join(INSTALLED_PACKAGES_LOCATION);
        let install_path = install_dir.join("library_busybox_latest");

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

        file_renamer
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
            pkg_downloader: downloader,
            directory_manager,
            file_renamer,
            base_path: PathBuf::from("/tmp/base"),
        };
        let result = pm.install(&agent_id, reference);

        assert!(matches!(result, Err(OCIPackageManagerError::Directory(_))));
    }
}
