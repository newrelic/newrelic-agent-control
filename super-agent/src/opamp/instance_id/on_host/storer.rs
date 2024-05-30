use crate::opamp::instance_id::getter::DataStored;
use crate::opamp::instance_id::storer::InstanceIDStorer;
use crate::super_agent::config::AgentID;
use crate::super_agent::defaults::{REMOTE_AGENT_DATA_DIR, SUPER_AGENT_IDENTIFIERS_PATH};
use fs::directory_manager::{DirectoryManagementError, DirectoryManager, DirectoryManagerFs};
use fs::file_reader::{FileReader, FileReaderError};
use fs::writer_file::{FileWriter, WriteError};
use fs::LocalFile;
use std::fs::Permissions;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tracing::debug;

#[cfg(target_family = "unix")]
const FILE_PERMISSIONS: u32 = 0o600;
#[cfg(target_family = "unix")]
const DIRECTORY_PERMISSIONS: u32 = 0o700;

#[derive(Default)]
pub struct Storer<F = LocalFile, D = DirectoryManagerFs>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    file_rw: F,
    dir_manager: D,
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

fn get_uuid_path(agent_id: &AgentID) -> PathBuf {
    if agent_id.is_super_agent_id() {
        PathBuf::from(SUPER_AGENT_IDENTIFIERS_PATH())
    } else {
        PathBuf::from(format!(
            "{}/{}/identifiers.yaml",
            REMOTE_AGENT_DATA_DIR(),
            agent_id.get()
        ))
    }
}

impl<F, D> InstanceIDStorer for Storer<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    fn set(&self, agent_id: &AgentID, ds: &DataStored) -> Result<(), StorerError> {
        self.write_contents(agent_id, ds)
    }

    fn get(&self, agent_id: &AgentID) -> Result<Option<DataStored>, StorerError> {
        self.read_contents(agent_id)
    }
}

impl<F, D> Storer<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    pub fn new(file_rw: F, dir_manager: D) -> Self {
        Self {
            file_rw,
            dir_manager,
        }
    }
}

impl<F, D> Storer<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    fn write_contents(&self, agent_id: &AgentID, ds: &DataStored) -> Result<(), StorerError> {
        let dest_file = get_uuid_path(agent_id);
        // Get a ref to the target file's parent directory
        let dest_dir = dest_file
            .parent()
            .expect("no parent directory found for {dest_file} (empty or root dir)");

        self.dir_manager
            .create(dest_dir, Permissions::from_mode(DIRECTORY_PERMISSIONS))?;
        let contents = serde_yaml::to_string(ds)?;

        Ok(self.file_rw.write(
            &dest_file,
            contents,
            Permissions::from_mode(FILE_PERMISSIONS),
        )?)
    }

    fn read_contents(&self, agent_id: &AgentID) -> Result<Option<DataStored>, StorerError> {
        let dest_path = get_uuid_path(agent_id);
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
}

#[cfg(test)]
mod test {
    use crate::opamp::instance_id::getter::DataStored;
    use crate::opamp::instance_id::on_host::storer::get_uuid_path;
    use crate::opamp::instance_id::storer::InstanceIDStorer;
    use crate::opamp::instance_id::{Identifiers, InstanceID, Storer};
    use crate::super_agent::config::AgentID;
    use crate::super_agent::defaults::{REMOTE_AGENT_DATA_DIR, SUPER_AGENT_IDENTIFIERS_PATH};
    use fs::directory_manager::mock::MockDirectoryManagerMock;
    use fs::mock::MockLocalFile;
    use mockall::predicate;
    use std::fs::Permissions;
    use std::io::{self, ErrorKind};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    #[test]
    fn basic_get_uild_path() {
        let agent_id = AgentID::new("test").unwrap();
        let path = get_uuid_path(&agent_id);
        assert_eq!(
            path,
            PathBuf::from(format!("{}/test/identifiers.yaml", REMOTE_AGENT_DATA_DIR()))
        );

        let super_agent_id = AgentID::new_super_agent_id();
        let path = get_uuid_path(&super_agent_id);
        assert_eq!(path, PathBuf::from(SUPER_AGENT_IDENTIFIERS_PATH()));
    }

    #[test]
    fn test_successful_write() {
        // Data
        let agent_id = AgentID::new("test").unwrap();
        let mut file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManagerMock::default();
        let ds = DataStored {
            uuid: InstanceID::new(uuid::Uuid::now_v7()),
            identifiers: test_identifiers(),
        };

        let uuid_path = get_uuid_path(&agent_id);

        // Expectations
        dir_manager.should_create(uuid_path.parent().unwrap(), Permissions::from_mode(0o700));
        file_rw.should_write(&uuid_path, expected_file(), Permissions::from_mode(0o600));

        let storer = Storer::new(file_rw, dir_manager);
        assert!(storer.set(&agent_id, &ds).is_ok());
    }

    #[test]
    fn test_unsuccessful_write() {
        // Data
        let agent_id = AgentID::new("test").unwrap();
        let instance_uuid = uuid::Uuid::now_v7();
        let mut file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManagerMock::default();
        let ds = DataStored {
            uuid: InstanceID::new(instance_uuid),
            identifiers: test_identifiers(),
        };

        let uuid_path = get_uuid_path(&agent_id);

        // Expectations
        file_rw.should_not_write(&uuid_path, expected_file(), Permissions::from_mode(0o600));
        dir_manager.should_create(uuid_path.parent().unwrap(), Permissions::from_mode(0o700));

        let storer = Storer::new(file_rw, dir_manager);
        assert!(storer.set(&agent_id, &ds).is_err());
    }

    #[test]
    fn test_successful_read() {
        // Data
        let agent_id = AgentID::new("test").unwrap();
        let mut file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManagerMock::default();
        let ds = DataStored {
            uuid: InstanceID::new(uuid::Uuid::now_v7()),
            identifiers: test_identifiers(),
        };
        let expected = Some(ds.clone());
        let uuid_path = get_uuid_path(&agent_id);

        // Expectations
        file_rw
            .expect_read()
            .with(predicate::function(move |p| p == uuid_path.as_path()))
            .once()
            .return_once(|_| Ok(expected_file()));

        let storer = Storer::new(file_rw, dir_manager);
        let actual = storer.get(&agent_id);
        assert!(actual.is_ok());
        assert_eq!(expected, actual.unwrap());
    }

    #[test]
    fn test_unsuccessful_read() {
        let agent_id = AgentID::new("test").unwrap();
        let mut file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManagerMock::default();
        let uuid_path = get_uuid_path(&agent_id);

        file_rw
            .expect_read()
            .with(predicate::function(move |p| p == uuid_path.as_path()))
            .once()
            .return_once(|_| Err(io::Error::new(ErrorKind::Other, "some error message").into()));

        let storer = Storer::new(file_rw, dir_manager);
        let expected = storer.get(&agent_id);

        // As said above, we are not generatinc the error variant here
        assert!(expected.is_ok());
        assert!(expected.unwrap().is_none());
    }

    /// HELPERS

    fn expected_file() -> String {
        String::from("uuid: test-UUID\nidentifiers:\n  hostname: test-hostname\n  machine_id: test-machine-id\n  cloud_instance_id: test-instance-id\n  host_id: test-host-id\n  fleet_id: test-fleet-id\n")
    }

    fn test_identifiers() -> Identifiers {
        Identifiers {
            hostname: "test-hostname".to_string(),
            machine_id: "test-machine-id".to_string(),
            cloud_instance_id: "test-instance-id".to_string(),
            host_id: "test-host-id".to_string(),
            fleet_id: "test-fleet-id".to_string(),
        }
    }
}
