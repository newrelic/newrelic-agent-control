use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::IDENTIFIERS_FILENAME;
use crate::opamp::instance_id::getter::DataStored;
use crate::opamp::instance_id::storer::InstanceIDStorer;
use fs::LocalFile;
use fs::directory_manager::{DirectoryManagementError, DirectoryManager, DirectoryManagerFs};
use fs::file_reader::{FileReader, FileReaderError};
use fs::utils::FsError;
use fs::writer_file::{FileWriter, WriteError};
use std::fs::Permissions;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tracing::debug;

use super::getter::Identifiers;

#[cfg(target_family = "unix")]
const FILE_PERMISSIONS: u32 = 0o600;
#[cfg(target_family = "unix")]
const DIRECTORY_PERMISSIONS: u32 = 0o700;

pub struct Storer<F = LocalFile, D = DirectoryManagerFs>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    file_rw: F,
    dir_manager: D,
    agent_control_remote_dir: PathBuf,
    agent_remote_dir: PathBuf,
}

#[derive(thiserror::Error, Debug)]
pub enum StorerError {
    #[error("Generic error")]
    Generic,
    #[error("Error (de)serializing from/into an identifiers file: `{0}`")]
    Serde(#[from] serde_yaml::Error),
    #[error("Directory management error: `{0}`")]
    DirectoryManagement(#[from] DirectoryManagementError),
    #[error("error writing file: `{0}`")]
    WriteError(#[from] WriteError),
    #[error("error creating file: `{0}`")]
    IOError(#[from] io::Error),
    #[error("error reading file: `{0}`")]
    ReadError(#[from] FileReaderError),
}

impl<F, D> InstanceIDStorer for Storer<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    type Error = StorerError;
    type Identifiers = Identifiers;
    fn set(
        &self,
        agent_id: &AgentID,
        ds: &DataStored<Self::Identifiers>,
    ) -> Result<(), Self::Error> {
        self.write_contents(agent_id, ds)
    }

    fn get(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<DataStored<Self::Identifiers>>, Self::Error> {
        self.read_contents(agent_id)
    }
}

impl<F, D> Storer<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    pub fn new(
        file_rw: F,
        dir_manager: D,
        agent_control_remote_dir: PathBuf,
        agent_remote_dir: PathBuf,
    ) -> Self {
        Self {
            file_rw,
            dir_manager,
            agent_control_remote_dir,
            agent_remote_dir,
        }
    }
}

impl<F, D> Storer<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    fn write_contents(
        &self,
        agent_id: &AgentID,
        ds: &DataStored<Identifiers>,
    ) -> Result<(), StorerError> {
        let dest_file = self.get_instance_id_path(agent_id);
        // Get a ref to the target file's parent directory
        let dest_dir = dest_file
            .parent()
            .ok_or(WriteError::from(FsError::InvalidPath(format!(
                "no parent directory found for {} (empty or root dir)",
                dest_file.display()
            ))))?;

        self.dir_manager
            .create(dest_dir, Permissions::from_mode(DIRECTORY_PERMISSIONS))?;
        let contents = serde_yaml::to_string(ds)?;

        Ok(self.file_rw.write(
            &dest_file,
            contents,
            Permissions::from_mode(FILE_PERMISSIONS),
        )?)
    }

    fn read_contents(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<DataStored<Identifiers>>, StorerError> {
        let dest_path = self.get_instance_id_path(agent_id);
        let file_str = match self.file_rw.read(dest_path.as_path()) {
            Ok(s) => s,
            Err(e) => {
                debug!("error reading file for agent {}: {}", agent_id, e);
                return Ok(None);
            }
        };
        match serde_yaml::from_str(&file_str) {
            Ok(ds) => Ok(Some(ds)),
            Err(e) => {
                debug!("error deserializing data for agent {}: {}", agent_id, e);
                Ok(None)
            }
        }
    }

    fn get_instance_id_path(&self, agent_id: &AgentID) -> PathBuf {
        if agent_id.is_agent_control_id() {
            self.agent_control_remote_dir.join(IDENTIFIERS_FILENAME)
        } else {
            self.agent_remote_dir
                .join(agent_id)
                .join("identifiers.yaml")
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::agent_control::agent_id::AgentID;
    use crate::opamp::instance_id::InstanceID;
    use crate::opamp::instance_id::getter::DataStored;
    use crate::opamp::instance_id::on_host::getter::Identifiers;
    use crate::opamp::instance_id::on_host::storer::Storer;
    use crate::opamp::instance_id::storer::InstanceIDStorer;
    use fs::directory_manager::mock::MockDirectoryManager;
    use fs::mock::MockLocalFile;
    use mockall::predicate;
    use std::fs::Permissions;
    use std::io::{self};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    #[test]
    fn basic_get_uild_path() {
        let sa_dir = PathBuf::from("/super");
        let sub_agent_dir = PathBuf::from("/sub");
        let storer = Storer::new(
            MockLocalFile::default(),
            MockDirectoryManager::default(),
            sa_dir.clone(),
            sub_agent_dir.clone(),
        );
        let agent_id = AgentID::new("test").unwrap();
        let path = storer.get_instance_id_path(&agent_id);
        assert_eq!(path, sub_agent_dir.join("test").join("identifiers.yaml"));

        let agent_control_id = AgentID::new_agent_control_id();
        let path = storer.get_instance_id_path(&agent_control_id);
        assert_eq!(path, sa_dir.join("identifiers.yaml"));
    }

    #[test]
    fn test_successful_write() {
        // Data
        let (agent_id, sa_path, sub_agent_path, instance_id_path) = test_data();
        let mut file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManager::default();
        let instance_id = InstanceID::create();
        let ds = DataStored {
            instance_id: instance_id.clone(),
            identifiers: test_identifiers(),
        };

        // Expectations
        dir_manager.should_create(
            instance_id_path.parent().unwrap(),
            Permissions::from_mode(0o700),
        );
        file_rw.should_write(
            &instance_id_path,
            expected_file(instance_id),
            Permissions::from_mode(0o600),
        );

        let storer = Storer::new(file_rw, dir_manager, sa_path, sub_agent_path);
        assert!(storer.set(&agent_id, &ds).is_ok());
    }

    #[test]
    fn test_unsuccessful_write() {
        // Data
        let (agent_id, sa_path, sub_agent_path, instance_id_path) = test_data();
        let mut file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManager::default();
        let instance_id = InstanceID::create();
        let ds = DataStored {
            instance_id: instance_id.clone(),
            identifiers: test_identifiers(),
        };

        // Expectations
        file_rw.should_not_write(
            &instance_id_path,
            expected_file(instance_id),
            Permissions::from_mode(0o600),
        );
        dir_manager.should_create(
            instance_id_path.parent().unwrap(),
            Permissions::from_mode(0o700),
        );

        let storer = Storer::new(file_rw, dir_manager, sa_path, sub_agent_path);
        assert!(storer.set(&agent_id, &ds).is_err());
    }

    #[test]
    fn test_successful_read() {
        // Data
        let (agent_id, sa_path, sub_agent_path, instance_id_path) = test_data();
        let mut file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManager::default();
        let instance_id = InstanceID::create();
        let ds = DataStored {
            instance_id: instance_id.clone(),
            identifiers: test_identifiers(),
        };
        let expected = Some(ds.clone());

        // Expectations
        file_rw
            .expect_read()
            .with(predicate::function(move |p| {
                p == instance_id_path.as_path()
            }))
            .once()
            .return_once(|_| Ok(expected_file(instance_id)));

        let storer = Storer::new(file_rw, dir_manager, sa_path, sub_agent_path);
        let actual = storer.get(&agent_id);
        assert!(actual.is_ok());
        assert_eq!(expected, actual.unwrap());
    }

    #[test]
    fn test_unsuccessful_read() {
        let (agent_id, sa_path, sub_agent_path, instance_id_path) = test_data();
        let mut file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManager::default();

        file_rw
            .expect_read()
            .with(predicate::function(move |p| {
                p == instance_id_path.as_path()
            }))
            .once()
            .return_once(|_| Err(io::Error::other("some error message").into()));

        let storer = Storer::new(file_rw, dir_manager, sa_path, sub_agent_path);
        let expected = storer.get(&agent_id);

        // As said above, we are not generating the error variant here
        assert!(expected.is_ok());
        assert!(expected.unwrap().is_none());
    }

    // HELPERS

    const HOSTNAME: &str = "test-hostname";
    const MICHINE_ID: &str = "test-machine-id";
    const CLOUD_INSTANCE_ID: &str = "test-instance-id";
    const HOST_ID: &str = "test-host-id";
    const FLEET_ID: &str = "test-fleet-id";

    fn test_data() -> (AgentID, PathBuf, PathBuf, PathBuf) {
        let agent_id = AgentID::new("test").unwrap();
        let sa_path = PathBuf::from("/super");
        let sub_agent_path = PathBuf::from("/sub");
        let instance_id_path = PathBuf::from("/sub/test/identifiers.yaml");
        (agent_id, sa_path, sub_agent_path, instance_id_path)
    }

    fn expected_file(instance_id: InstanceID) -> String {
        format!(
            "instance_id: {instance_id}\nidentifiers:\n  hostname: {HOSTNAME}\n  machine_id: {MICHINE_ID}\n  cloud_instance_id: {CLOUD_INSTANCE_ID}\n  host_id: {HOST_ID}\n  fleet_id: {FLEET_ID}\n",
        )
    }

    fn test_identifiers() -> Identifiers {
        Identifiers {
            hostname: HOSTNAME.into(),
            machine_id: MICHINE_ID.into(),
            cloud_instance_id: CLOUD_INSTANCE_ID.into(),
            host_id: HOST_ID.into(),
            fleet_id: FLEET_ID.into(),
        }
    }
}
