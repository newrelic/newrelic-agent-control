use super::downloader::{OCIAgentDownloader, OCIDownloaderError};
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::PACKAGES_FOLDER_NAME;
use crate::agent_type::runtime_config::on_host::package::PackageID;
use crate::package::manager::{InstalledPackageData, PackageData, PackageManager};
use crate::package::oci::artifact_definitions::LocalAgentPackage;
use crate::package::oci::downloader::OCIArtifactDownloader;
use fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use fs::file::LocalFile;
use fs::file::reader::FileReader;
use oci_client::Reference;
use std::collections::HashMap;
use std::fmt::Display;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
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
    latest_installed_packages: Mutex<HashMap<AgentID, InstalledPackageData>>,
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
    #[error("errors removing packages: {0}")]
    RetainPackageErrors(RetainPackageErrors),
}

#[derive(Debug, Default)]
pub struct RetainPackageErrors(Vec<(String, OCIPackageManagerError)>);
impl RetainPackageErrors {
    pub fn push(&mut self, package_id: String, error: OCIPackageManagerError) {
        self.0.push((package_id, error));
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
impl Display for RetainPackageErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let errors = self
            .0
            .iter()
            .map(|(package_id, error)| format!("package_id: {package_id}, error: {error}"))
            .reduce(|acc, s| format!("{acc}, {s}"))
            .unwrap_or_default();
        write!(f, "[{errors}]")?;
        Ok(())
    }
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
            latest_installed_packages: Mutex::new(HashMap::new()),
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
            .download(
                &package_data.oci_reference,
                &package_data.public_key_url,
                tmp_download_path,
            )
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

    /// Retains the given installed package and the previously retained one, uninstalling any other installed packages for the agent.
    fn retain_packages(
        &self,
        agent_id: &AgentID,
        installed_package: InstalledPackageData,
    ) -> Result<(), OCIPackageManagerError> {
        let mut pkg_to_retain = vec![installed_package.clone()];
        if let Some(previous_package) = self
            .latest_installed_packages
            .lock()
            .expect("fail to acquire lock")
            .insert(agent_id.clone(), installed_package)
        {
            pkg_to_retain.push(previous_package);
        };

        let installed_packages = self.installed_packages(agent_id)?;

        let mut errors = RetainPackageErrors::default();
        for pck_to_remove in installed_packages {
            if pkg_to_retain.contains(&pck_to_remove) {
                continue;
            }
            debug!(
                "Removing {} package {}",
                pck_to_remove.id,
                pck_to_remove.installation_path.display()
            );
            if let Err(e) = self.uninstall(agent_id, pck_to_remove.clone()) {
                errors.push(pck_to_remove.id, e);
            }
        }
        if !errors.is_empty() {
            return Err(OCIPackageManagerError::RetainPackageErrors(errors));
        }

        Ok(())
    }

    /// Lists all installed packages for the given agent.
    fn installed_packages(
        &self,
        agent_id: &AgentID,
    ) -> Result<Vec<InstalledPackageData>, OCIPackageManagerError> {
        let installed_packages_dir =
            get_generic_package_location_path(&self.remote_dir, agent_id, INSTALLED_PCK_LOCATION)?;

        let mut installed_packages = vec![];
        let id_dirs = LocalFile
            .dir_entries(&installed_packages_dir)
            .map_err(OCIPackageManagerError::Install)?;

        for id_path in id_dirs {
            if !id_path.is_dir() {
                debug!(
                    "Unexpected file found on packages id dir: {}",
                    id_path.display()
                );
                continue;
            }

            let Some(package_id) = id_path.file_name().map(|name| name.to_string_lossy()) else {
                debug!(
                    "Unexpected file found on packages id dir: {}",
                    id_path.display()
                );
                continue;
            };
            let package_dirs = LocalFile
                .dir_entries(&id_path)
                .map_err(OCIPackageManagerError::Install)?;
            for package_dir in package_dirs {
                if !package_dir.is_dir() {
                    debug!(
                        "Unexpected file found on packages dir: {}",
                        package_dir.display()
                    );
                    continue;
                }
                installed_packages.push(InstalledPackageData {
                    id: package_id.to_string(),
                    installation_path: package_dir,
                });
            }
        }
        Ok(installed_packages)
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
    let package_id_path = get_generic_package_location_path(base_path, agent_id, location)?;
    Ok(package_id_path
        .join(package_id)
        .join(compute_path_suffix(package_reference)?))
}
fn get_generic_package_location_path(
    base_path: &Path,
    agent_id: &AgentID,
    location: &str,
) -> Result<PathBuf, OCIPackageManagerError> {
    Ok(base_path
        .join(PACKAGES_FOLDER_NAME)
        .join(agent_id)
        .join(location))
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
    ///
    /// # Package Retention Policy
    /// The Package Manager keeps track of the latest installed package. Each install operation executes
    /// an older packages purge operation. The purge operation retains the latest tracked package (current in execution)
    /// and new installed. The main goal of this is to avoid a never ending disk usage growth on package updates.
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

            let installed_package = InstalledPackageData {
                id: package_data.id,
                installation_path: package_path,
            };

            self.retain_packages(agent_id, installed_package.clone())?;

            return Ok(installed_package);
        }

        let temp_package_path = get_temp_package_path(
            &self.remote_dir,
            agent_id,
            &package_data.id,
            &package_data.oci_reference,
        )?;

        // If we face an error during installation, we must ensure the temporary directory is deleted.
        // We hide the error of the folder if something else went wrong.
        let installed_package = self
            .install_archive(&package_data, &temp_package_path, &package_path)
            .inspect_err(|_| _ = self.directory_manager.delete(&temp_package_path))?;

        self.directory_manager
            .delete(&temp_package_path)
            .map_err(OCIPackageManagerError::Install)?;

        self.retain_packages(agent_id, installed_package.clone())?;

        Ok(installed_package)
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
    use crate::package::oci::artifact_definitions::PackageMediaType;
    use crate::package::oci::downloader::tests::MockOCIDownloader;
    use crate::utils::extract::tests::TestDataHelper;
    use fs::directory_manager::mock::MockDirectoryManager;
    use fs::file::writer::FileWriter;
    use mockall::predicate::eq;
    use oci_spec::distribution::Reference;
    use std::str::FromStr;
    use tempfile::tempdir;

    const TEST_PACKAGE_ID: &str = "test-package";

    fn test_reference() -> Reference {
        Reference::from_str("docker.io/library/busybox:latest@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef").unwrap()
    }

    fn test_package_data() -> PackageData {
        PackageData {
            id: TEST_PACKAGE_ID.to_string(),
            oci_reference: test_reference(),
            public_key_url: None,
        }
    }

    fn new_local_package(path: &Path) -> LocalAgentPackage {
        LocalAgentPackage::new(PackageMediaType::AgentPackageLayerTarGz, path.to_path_buf())
    }

    fn new_package_version(version: &str) -> PackageData {
        PackageData {
            id: TEST_PACKAGE_ID.to_string(),
            oci_reference: Reference::from_str(format!("newrelic/fake-agent:{}", version).as_str())
                .unwrap(),
            public_key_url: None,
        }
    }

    fn fake_compressed_package(download_dir: &Path) -> LocalAgentPackage {
        DirectoryManagerFs.create(download_dir).unwrap();
        let downloaded_file = download_dir.join("layer_digest.tar.gz");
        let tmp_dir_to_compress = tempdir().unwrap();
        TestDataHelper::compress_tar_gz(tmp_dir_to_compress.path(), downloaded_file.as_path());

        new_local_package(&downloaded_file)
    }

    #[test]
    fn test_removes_untracked_packages_on_install() {
        let mut downloader = MockOCIDownloader::new();
        let agent_id = AgentID::try_from("agent-id").unwrap();

        let root_dir = tempdir().unwrap();

        downloader
            .expect_download()
            .times(1)
            .returning(move |_, _, download_dir| Ok(fake_compressed_package(download_dir)));

        let pm = OCIPackageManager::new(
            downloader,
            DirectoryManagerFs,
            PathBuf::from(root_dir.path()),
        );

        let untracked_package_path = get_package_path(
            root_dir.path(),
            &agent_id,
            &TEST_PACKAGE_ID.to_string(),
            &Reference::from_str("newrelic/fake-agent:v0").unwrap(),
        )
        .unwrap();
        DirectoryManagerFs.create(&untracked_package_path).unwrap();

        assert!(untracked_package_path.exists());

        let old_package = pm.install(&agent_id, new_package_version("v1")).unwrap();
        assert!(!untracked_package_path.exists());
        assert!(old_package.installation_path.exists());
    }

    #[test]
    fn test_removes_only_packages_from_agent_id() {
        let mut downloader = MockOCIDownloader::new();
        let agent_id = AgentID::try_from("agent-id").unwrap();

        let root_dir = tempdir().unwrap();

        downloader
            .expect_download()
            .times(1)
            .returning(move |_, _, download_dir| Ok(fake_compressed_package(download_dir)));

        let pm = OCIPackageManager::new(
            downloader,
            DirectoryManagerFs,
            PathBuf::from(root_dir.path()),
        );

        // Existing package from a different agent id should not be removed
        let other_agent_package = get_package_path(
            root_dir.path(),
            &AgentID::try_from("other-agent-id").unwrap(),
            &TEST_PACKAGE_ID.to_string(),
            &Reference::from_str("newrelic/fake-agent:v0").unwrap(),
        )
        .unwrap();
        DirectoryManagerFs.create(&other_agent_package).unwrap();

        // Spurious file at installed packages level should not be removed
        let pkg_id_dir =
            get_generic_package_location_path(root_dir.path(), &agent_id, INSTALLED_PCK_LOCATION)
                .unwrap();
        DirectoryManagerFs.create(&pkg_id_dir).unwrap();
        let pkg_id_level_spurious_file = pkg_id_dir.join("pkg_id_spurious_file");
        LocalFile
            .write(&pkg_id_level_spurious_file, "content".to_string())
            .unwrap();

        let current_package = pm.install(&agent_id, new_package_version("v1")).unwrap();
        assert!(current_package.installation_path.exists());
        assert!(other_agent_package.exists());
        assert!(pkg_id_level_spurious_file.exists());
    }

    #[test]
    fn test_removes_older_packages_when_new_installs() {
        let mut downloader = MockOCIDownloader::new();
        let agent_id = AgentID::try_from("agent-id").unwrap();

        let root_dir = tempdir().unwrap();

        downloader
            .expect_download()
            .times(3)
            .returning(move |_, _, download_dir| Ok(fake_compressed_package(download_dir)));

        let pm = OCIPackageManager::new(
            downloader,
            DirectoryManagerFs,
            PathBuf::from(root_dir.path()),
        );

        let old_package = pm.install(&agent_id, new_package_version("v1")).unwrap();
        assert!(old_package.installation_path.exists());

        let previous_package = pm.install(&agent_id, new_package_version("v2")).unwrap();
        assert!(old_package.installation_path.exists());
        assert!(previous_package.installation_path.exists());

        let current_package = pm.install(&agent_id, new_package_version("v3")).unwrap();
        assert!(!old_package.installation_path.exists());
        assert!(previous_package.installation_path.exists());
        assert!(current_package.installation_path.exists());
    }

    #[test]
    fn test_supports_rollback() {
        let mut downloader = MockOCIDownloader::new();
        let agent_id = AgentID::try_from("agent-id").unwrap();

        let root_dir = tempdir().unwrap();

        downloader
            .expect_download()
            .times(3)
            .returning(move |_, _, download_dir| Ok(fake_compressed_package(download_dir)));

        let pm = OCIPackageManager::new(
            downloader,
            DirectoryManagerFs,
            PathBuf::from(root_dir.path()),
        );

        let v1_package = pm.install(&agent_id, new_package_version("v1")).unwrap();
        assert!(v1_package.installation_path.exists());

        let v2_package = pm.install(&agent_id, new_package_version("v2")).unwrap();
        assert!(v1_package.installation_path.exists());
        assert!(v2_package.installation_path.exists());

        let v1_rollback_package = pm.install(&agent_id, new_package_version("v1")).unwrap();
        assert!(v1_package.installation_path.exists());
        assert!(v2_package.installation_path.exists());
        assert!(v1_rollback_package.installation_path.exists());

        let v3_package = pm.install(&agent_id, new_package_version("v3")).unwrap();
        assert!(v3_package.installation_path.exists());
        assert!(v1_package.installation_path.exists());
        assert!(v1_rollback_package.installation_path.exists());
        assert!(!v2_package.installation_path.exists());
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
            .with(eq(test_reference()), eq(None), eq(download_dir.clone()))
            .once()
            .returning(move |_, _, _| Ok(fake_compressed_package(&download_dir)));

        let pm = OCIPackageManager::new(
            downloader,
            DirectoryManagerFs,
            PathBuf::from(root_dir.path()),
        );
        let package_data = test_package_data();
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
            .with(eq(test_reference()), eq(None), eq(download_dir.clone()))
            .once()
            .returning(move |_, _, _| {
                // Mock downloader behavior creating a compressed file with known content, but WRONG FORMAT
                DirectoryManagerFs.create(&download_dir).unwrap();
                let downloaded_file = download_dir.join("layer_digest.tar.gz");
                let tmp_dir_to_compress = tempdir().unwrap();
                TestDataHelper::compress_zip(tmp_dir_to_compress.path(), downloaded_file.as_path());

                Ok(new_local_package(&downloaded_file))
            });

        let pm = OCIPackageManager::new(
            downloader,
            DirectoryManagerFs,
            PathBuf::from(root_dir.path()),
        );

        let package_data = test_package_data();
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

        let pm = OCIPackageManager::new(downloader, directory_manager, remote_dir);

        let package_data = test_package_data();

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
            .with(eq(test_reference()), eq(None), eq(download_dir))
            .once()
            .returning(|_, _, _| {
                Err(OCIDownloaderError::DownloadingArtifact(
                    "download failed".into(),
                ))
            });

        let pm = OCIPackageManager::new(downloader, directory_manager, remote_dir);

        let package_data = test_package_data();
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
            .with(eq(test_reference()), eq(None), eq(download_dir.clone()))
            .once()
            .returning(move |_, _, _| Ok(new_local_package(&downloaded_file)));

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

        let pm = OCIPackageManager::new(downloader, directory_manager, remote_dir);

        let package_data = PackageData {
            id: TEST_PACKAGE_ID.to_string(),
            oci_reference: test_reference(),
            public_key_url: None,
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
            .with(eq(test_reference()), eq(None), eq(download_dir.clone()))
            .once()
            .returning(move |_, _, _| Ok(fake_compressed_package(&download_dir_copy)));

        directory_manager
            .expect_delete()
            .with(eq(download_dir.clone()))
            .once()
            .returning(|_| Err(io::Error::other("error deleting directory")));

        let pm = OCIPackageManager::new(downloader, directory_manager, remote_dir);

        let package_data = PackageData {
            id: TEST_PACKAGE_ID.to_string(),
            oci_reference: test_reference(),
            public_key_url: None,
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

        let pm = OCIPackageManager::new(downloader, directory_manager, PathBuf::from("/tmp/base"));
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

        let pm = OCIPackageManager::new(
            downloader,
            DirectoryManagerFs,
            PathBuf::from(remote_dir.path()),
        );

        let package_data = PackageData {
            id: TEST_PACKAGE_ID.to_string(),
            oci_reference: test_reference(),
            public_key_url: None,
        };

        let installed = pm.install(&agent_id, package_data).unwrap();

        assert_eq!(installed.installation_path, install_dir);
        assert_eq!(installed.id, TEST_PACKAGE_ID);
    }
}
