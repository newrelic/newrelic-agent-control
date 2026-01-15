use super::downloader::{OCIAgentDownloader, OCIDownloaderError};
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::PACKAGES_FOLDER_NAME;
use crate::agent_type::runtime_config::on_host::package::PackageID;
use crate::package::manager::{InstalledPackageData, PackageData, PackageManager};
use crate::package::oci::artifact_definitions::LocalAgentPackage;
use crate::package::oci::downloader::OCIArtifactDownloader;
use fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use oci_client::Reference;
use std::io;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;
use tracing::{debug, error, warn};

pub type DefaultOCIPackageManager = OCIPackageManager<OCIArtifactDownloader, DirectoryManagerFs>;

// This is expected to be thread-safe
pub struct OCIPackageManager<D, DM>
where
    D: OCIAgentDownloader,
    DM: DirectoryManager,
{
    downloader: D,
    directory_manager: DM,
    remote_dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum OCIPackageManagerError {
    #[error("error attempting to download OCI artifact: {0}")]
    Download(OCIDownloaderError),
    #[error("error attempting to install OCI artifact: {0}")]
    Install(io::Error),
    #[error("error attempting to uninstall OCI artifact: {0}")]
    Uninstall(io::Error),
    #[error("error extracting archive while installing OCI artifact: {0}")]
    Extraction(String),
    // Naming produces a non-normalized suffix. Should not happen but we can identify bugs with it.
    #[error("Package reference naming validation produces a non-normalized suffix: {0}")]
    NotNormalSuffix(String),
}

const TEMP_PCK_LOCATION: &str = "__temp_packages";
const INSTALLED_PCK_LOCATION: &str = "stored_packages";

impl<D, DM> OCIPackageManager<D, DM>
where
    D: OCIAgentDownloader,
    DM: DirectoryManager,
{
    pub fn new(downloader: D, directory_manager: DM, remote_dir: PathBuf) -> Self {
        Self {
            downloader,
            directory_manager,
            remote_dir,
        }
    }

    /// Downloads and installs the OCI package specified in `package_data`.
    /// The package is first downloaded to `temp_package_path` and then extracted to `package_path`.
    fn install_archive(
        &self,
        package_data: &PackageData,
        tmp_download_path: &Path,
        install_path: &Path,
    ) -> Result<InstalledPackageData, OCIPackageManagerError> {
        self.directory_manager
            .create(tmp_download_path)
            .map_err(OCIPackageManagerError::Install)?;

        let downloaded_package = self
            .downloader
            .download(&package_data.oci_reference, tmp_download_path)
            .map_err(OCIPackageManagerError::Download)?;

        self.extract_package(&downloaded_package, install_path)
            .inspect_err(|e| warn!("OCI package installation failed: {}", e))?;

        debug!("OCI package installed at {}", install_path.display());
        Ok(InstalledPackageData {
            id: package_data.id.clone(),
            installation_path: install_path.to_path_buf(),
        })
    }

    /// Extract the downloaded package file from `download_filepath` to `extract_dir`.
    /// if the extraction is successful, returns the `extract_dir` path.
    /// otherwise if deletes the `extract_dir` and returns an error.
    fn extract_package(
        &self,
        package: &LocalAgentPackage,
        extract_dir: &Path,
    ) -> Result<(), OCIPackageManagerError> {
        // Build and create destination directory
        self.directory_manager
            .create(extract_dir)
            .map_err(OCIPackageManagerError::Install)?;

        package.extract(extract_dir).map_err(|e| {
            _ = self.directory_manager.delete(extract_dir).inspect_err(|e| {
                error!("Failed to delete extraction directory after extraction failure: {e}")
            });
            OCIPackageManagerError::Extraction(e.to_string())
        })?;

        debug!(
            "Package extraction succeeded. Written to {}",
            extract_dir.display()
        );
        Ok(())
    }
}

pub fn get_package_path(
    base_path: &Path,
    agent_id: &AgentID,
    pck_id: &PackageID,
    pck_ref: &Reference,
) -> Result<PathBuf, OCIPackageManagerError> {
    get_generic_package_path(base_path, agent_id, INSTALLED_PCK_LOCATION, pck_id, pck_ref)
}

fn get_temp_package_path(
    base_path: &Path,
    agent_id: &AgentID,
    pck_id: &PackageID,
    pck_ref: &Reference,
) -> Result<PathBuf, OCIPackageManagerError> {
    get_generic_package_path(base_path, agent_id, TEMP_PCK_LOCATION, pck_id, pck_ref)
}

fn get_generic_package_path(
    base_path: &Path,
    agent_id: &AgentID,
    location: &str,
    package_id: &PackageID,
    package_reference: &Reference,
) -> Result<PathBuf, OCIPackageManagerError> {
    Ok(base_path
        .join(PACKAGES_FOLDER_NAME)
        .join(agent_id)
        .join(location)
        .join(package_id)
        .join(compute_path_suffix(package_reference)?))
}

/// Computes the download destination of a package [`Reference`] depending on the available fields.
///
/// The path is computed by sanitizing the package reference string to ensure it is a valid filename
/// on both Windows and Unix, and to prevent path traversal or injection.
///
/// The sanitization process:
/// 1. Prepends "oci_" to the filename to avoid reserved filenames (e.g. "CON" on Windows) and
///    to prevent the filename from being exactly "." or "..".
/// 2. Replaces directory separators (`/`, `\`) with `__`.
/// 3. Replaces any other character that is not alphanumeric, `.`, `-`, `_`, `@`, etc with `_`.
fn compute_path_suffix(package: &Reference) -> Result<PathBuf, OCIPackageManagerError> {
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

impl<D, DM> PackageManager for OCIPackageManager<D, DM>
where
    D: OCIAgentDownloader,
    DM: DirectoryManager,
{
    /// Installs the given OCI package for the specified agent.
    ///
    /// This method downloads the package to a temporary location and then extracts it to its final
    /// installation directory. The final location is determined based on the package reference.
    ///
    /// The temporary location is deleted before this function returns, regardless of the install
    /// success or failure.
    fn install(
        &self,
        agent_id: &AgentID,
        package_data: PackageData,
    ) -> Result<InstalledPackageData, OCIPackageManagerError> {
        // Using the whole reference (including tag/digest if available) with special chars replaces as the download path suffix (see function doc for details)
        let package_path = get_package_path(
            &self.remote_dir,
            agent_id,
            &package_data.id,
            &package_data.oci_reference,
        )?;

        if package_path.exists() {
            debug!(
                "Package already installed at {}. Skipping download and extraction.",
                package_path.display()
            );

            return Ok(InstalledPackageData {
                id: package_data.id,
                installation_path: package_path,
            });
        }

        let temp_package_path = get_temp_package_path(
            &self.remote_dir,
            agent_id,
            &package_data.id,
            &package_data.oci_reference,
        )?;

        // If we face an error during installation, we must ensure the temporary directory is deleted.
        // We hide the error of the folder if something else went wrong.
        let archive = self
            .install_archive(&package_data, &temp_package_path, &package_path)
            .inspect_err(|_| _ = self.directory_manager.delete(&temp_package_path))?;

        self.directory_manager
            .delete(&temp_package_path)
            .map_err(OCIPackageManagerError::Install)?;

        Ok(archive)
    }

    fn uninstall(
        &self,
        _agent_id: &AgentID,
        package: InstalledPackageData,
    ) -> Result<(), OCIPackageManagerError> {
        self.directory_manager
            .delete(&package.installation_path)
            .map_err(OCIPackageManagerError::Uninstall)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::oci::downloader::tests::MockOCIDownloader;
    use crate::utils::extract::tests::TestDataHelper;
    use fs::directory_manager::mock::MockDirectoryManager;
    use mockall::predicate::eq;
    use oci_spec::distribution::Reference;
    use std::str::FromStr;
    use tempfile::tempdir;

    const TEST_PACKAGE_ID: &str = "test-package";

    fn test_reference() -> Reference {
        Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap()
    }

    #[test]
    fn test_install_success() {
        let mut downloader = MockOCIDownloader::new();
        let agent_id = AgentID::try_from("agent-id").unwrap();

        let root_dir = tempdir().unwrap();
        let download_dir = get_temp_package_path(
            root_dir.path(),
            &agent_id,
            &TEST_PACKAGE_ID.to_string(),
            &test_reference(),
        )
        .unwrap();

        downloader
            .expect_download()
            .with(eq(test_reference()), eq(download_dir.clone()))
            .once()
            .returning(move |_, _| {
                // Mock downloader behavior creating a compressed file with known content
                DirectoryManagerFs {}.create(&download_dir).unwrap();
                let downloaded_file = download_dir.join("layer_digest.tar.gz");
                let tmp_dir_to_compress = tempdir().unwrap();
                TestDataHelper::compress_tar_gz(
                    tmp_dir_to_compress.path(),
                    downloaded_file.as_path(),
                );

                Ok(LocalAgentPackage::new(downloaded_file))
            });

        let pm = OCIPackageManager {
            downloader,
            directory_manager: DirectoryManagerFs {},
            remote_dir: PathBuf::from(root_dir.path()),
        };
        let package_data = PackageData {
            id: TEST_PACKAGE_ID.to_string(),
            oci_reference: test_reference(),
        };
        let installed = pm.install(&agent_id, package_data).unwrap();

        TestDataHelper::test_data_uncompressed(installed.installation_path.as_path());

        assert_eq!(installed.id, TEST_PACKAGE_ID.to_string());
    }

    #[test]
    fn test_install_extraction_failure() {
        let mut downloader = MockOCIDownloader::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();

        let root_dir = tempdir().unwrap();
        let download_dir = get_temp_package_path(
            root_dir.path(),
            &agent_id,
            &TEST_PACKAGE_ID.to_string(),
            &test_reference(),
        )
        .unwrap();

        downloader
            .expect_download()
            .with(eq(test_reference()), eq(download_dir.clone()))
            .once()
            .returning(move |_, _| {
                // Mock downloader behavior creating a compressed file with known content, but WRONG FORMAT
                DirectoryManagerFs {}.create(&download_dir).unwrap();
                let downloaded_file = download_dir.join("layer_digest.tar.gz");
                let tmp_dir_to_compress = tempdir().unwrap();
                TestDataHelper::compress_zip(tmp_dir_to_compress.path(), downloaded_file.as_path());

                Ok(LocalAgentPackage::new(downloaded_file))
            });

        let pm = OCIPackageManager {
            downloader,
            directory_manager: DirectoryManagerFs {},
            remote_dir: PathBuf::from(root_dir.path()),
        };

        let package_data = PackageData {
            id: TEST_PACKAGE_ID.to_string(),
            oci_reference: test_reference(),
        };
        let err = pm.install(&agent_id, package_data).unwrap_err();
        assert!(matches!(err, OCIPackageManagerError::Extraction(_)));
    }

    #[test]
    fn test_install_directory_creation_failure() {
        let downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();

        let temp_dir = tempdir().unwrap();
        let remote_dir = temp_dir.path().to_path_buf();

        let download_dir = get_temp_package_path(
            &remote_dir,
            &agent_id,
            &TEST_PACKAGE_ID.to_string(),
            &test_reference(),
        )
        .unwrap();

        directory_manager
            .expect_create()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Err(io::Error::other("error creating directory")));

        directory_manager
            .expect_delete()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            remote_dir,
        };
        let package_data = PackageData {
            id: TEST_PACKAGE_ID.to_string(),
            oci_reference: test_reference(),
        };
        let result = pm.install(&agent_id, package_data);

        assert!(matches!(result, Err(OCIPackageManagerError::Install(_))));
    }

    #[test]
    fn test_install_download_failure() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();

        let temp_dir = tempdir().unwrap();
        let remote_dir = temp_dir.path().to_path_buf();

        let download_dir = get_temp_package_path(
            &remote_dir,
            &agent_id,
            &TEST_PACKAGE_ID.to_string(),
            &test_reference(),
        )
        .unwrap();

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
            .with(eq(test_reference()), eq(download_dir))
            .once()
            .returning(|_, _| {
                Err(OCIDownloaderError::DownloadingArtifact(
                    "download failed".into(),
                ))
            });

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            remote_dir,
        };
        let package_data = PackageData {
            id: TEST_PACKAGE_ID.to_string(),
            oci_reference: test_reference(),
        };
        let result = pm.install(&agent_id, package_data);

        assert!(matches!(result, Err(OCIPackageManagerError::Download(_))));
    }

    #[test]
    fn test_install_final_directory_creation_failure() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();

        let temp_dir = tempdir().unwrap();
        let remote_dir = temp_dir.path().to_path_buf();

        let download_dir = get_temp_package_path(
            &remote_dir,
            &agent_id,
            &TEST_PACKAGE_ID.to_string(),
            &test_reference(),
        )
        .unwrap();

        let downloaded_file = download_dir.join("layer_digest.tar.gz");
        let install_dir = get_package_path(
            &remote_dir,
            &agent_id,
            &TEST_PACKAGE_ID.to_string(),
            &test_reference(),
        )
        .unwrap();

        directory_manager
            .expect_create()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        downloader
            .expect_download()
            .with(eq(test_reference()), eq(download_dir.clone()))
            .once()
            .returning(move |_, _| Ok(LocalAgentPackage::new(downloaded_file.clone())));

        directory_manager
            .expect_create()
            .with(eq(install_dir.clone()))
            .once()
            .returning(|_| Err(io::Error::other("error creating directory")));

        directory_manager
            .expect_delete()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Ok(()));

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            remote_dir,
        };
        let package_data = PackageData {
            id: TEST_PACKAGE_ID.to_string(),
            oci_reference: test_reference(),
        };
        let result = pm.install(&agent_id, package_data);

        assert!(matches!(result, Err(OCIPackageManagerError::Install(_))));
    }

    #[test]
    fn test_install_cleanup_failure() {
        let mut downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();

        let temp_dir = tempdir().unwrap();
        let remote_dir = temp_dir.path().to_path_buf();

        let download_dir = get_temp_package_path(
            &remote_dir,
            &agent_id,
            &TEST_PACKAGE_ID.to_string(),
            &test_reference(),
        )
        .unwrap();

        let install_dir = get_package_path(
            &remote_dir,
            &agent_id,
            &TEST_PACKAGE_ID.to_string(),
            &test_reference(),
        )
        .unwrap();

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

        let download_dir_copy = download_dir.clone();
        downloader
            .expect_download()
            .with(eq(test_reference()), eq(download_dir.clone()))
            .once()
            .returning(move |_, _| {
                // Mock downloader behavior creating a compressed file with known content
                DirectoryManagerFs {}.create(&download_dir_copy).unwrap();
                let downloaded_file = download_dir_copy.join("layer_digest.tar.gz");
                let tmp_dir_to_compress = tempdir().unwrap();
                TestDataHelper::compress_tar_gz(
                    tmp_dir_to_compress.path(),
                    downloaded_file.as_path(),
                );

                Ok(LocalAgentPackage::new(downloaded_file))
            });

        directory_manager
            .expect_delete()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Err(io::Error::other("error deleting directory")));

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            remote_dir,
        };
        let package_data = PackageData {
            id: TEST_PACKAGE_ID.to_string(),
            oci_reference: test_reference(),
        };
        let result = pm.install(&agent_id, package_data);

        assert!(matches!(result, Err(OCIPackageManagerError::Install(_))));
    }

    #[test]
    fn test_uninstall_success() {
        let downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let package_path = PathBuf::from("/path/to/package");

        directory_manager
            .expect_delete()
            .with(eq(package_path.clone()))
            .once()
            .returning(|_| Ok(()));

        let pm = OCIPackageManager {
            downloader,
            directory_manager,
            remote_dir: PathBuf::from("/tmp/base"),
        };
        let installed_package = InstalledPackageData {
            id: TEST_PACKAGE_ID.to_string(),
            installation_path: package_path,
        };
        let result = pm.uninstall(&agent_id, installed_package);

        assert!(result.is_ok());
    }

    #[test]
    fn test_uninstall_failure() {
        let downloader = MockOCIDownloader::new();
        let mut directory_manager = MockDirectoryManager::new();

        let agent_id = AgentID::try_from("agent-id").unwrap();
        let package_path = PathBuf::from("/path/to/package");

        directory_manager
            .expect_delete()
            .with(eq(package_path.clone()))
            .once()
            .returning(|_| Err(io::Error::other("error deleting directory")));

        let temp_dir = tempdir().unwrap();

        let pm =
            OCIPackageManager::new(downloader, directory_manager, temp_dir.path().to_path_buf());
        let installed_package = InstalledPackageData {
            id: TEST_PACKAGE_ID.to_string(),
            installation_path: package_path,
        };
        let result = pm.uninstall(&agent_id, installed_package);

        assert!(matches!(result, Err(OCIPackageManagerError::Uninstall(_))));
    }

    #[test]
    fn test_install_skips_download_if_already_installed() {
        let mut downloader = MockOCIDownloader::new();
        let agent_id = AgentID::try_from("agent-id").unwrap();
        let remote_dir = tempdir().unwrap();

        let install_dir = get_package_path(
            remote_dir.path(),
            &agent_id,
            &TEST_PACKAGE_ID.to_string(),
            &test_reference(),
        )
        .unwrap();

        std::fs::create_dir_all(&install_dir).expect("Failed to create dir");

        downloader.expect_download().times(0);

        let pm = OCIPackageManager {
            downloader,
            directory_manager: DirectoryManagerFs {},
            remote_dir: PathBuf::from(remote_dir.path()),
        };

        let package_data = PackageData {
            id: TEST_PACKAGE_ID.to_string(),
            oci_reference: test_reference(),
        };

        let installed = pm.install(&agent_id, package_data).unwrap();

        assert_eq!(installed.installation_path, install_dir);
        assert_eq!(installed.id, TEST_PACKAGE_ID);
    }
}
