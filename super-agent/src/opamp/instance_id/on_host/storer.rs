use crate::config::super_agent_configs::AgentID;
use crate::fs::directory_manager::{
    DirectoryManagementError, DirectoryManager, DirectoryManagerFs,
};
#[cfg_attr(test, mockall_double::double)]
use crate::fs::file_reader::FSFileReader;
use crate::fs::file_reader::FileReaderError;
use crate::fs::writer_file::WriteError;
#[cfg_attr(test, mockall_double::double)]
use crate::fs::writer_file::WriterFile;
use crate::opamp::instance_id::getter::DataStored;
use crate::opamp::instance_id::storer::InstanceIDStorer;

use crate::super_agent::defaults::{REMOTE_AGENT_DATA_DIR, SUPER_AGENT_IDENTIFIERS_PATH};
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
pub struct Storer<D = DirectoryManagerFs>
where
    D: DirectoryManager,
{
    file_writer: WriterFile,
    file_reader: FSFileReader,
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

fn get_uild_path(agent_id: &AgentID) -> PathBuf {
    if agent_id.is_super_agent_id() {
        PathBuf::from(SUPER_AGENT_IDENTIFIERS_PATH)
    } else {
        PathBuf::from(format!(
            "{}/{}/identifiers.yaml",
            REMOTE_AGENT_DATA_DIR,
            agent_id.get()
        ))
    }
}

impl<D> InstanceIDStorer for Storer<D>
where
    D: DirectoryManager,
{
    fn set(&self, agent_id: &AgentID, ds: &DataStored) -> Result<(), StorerError> {
        self.write_contents(agent_id, ds)
    }

    fn get(&self, agent_id: &AgentID) -> Result<Option<DataStored>, StorerError> {
        self.read_contents(agent_id)
    }
}

impl<D> Storer<D>
where
    D: DirectoryManager,
{
    pub fn new(file_writer: WriterFile, file_reader: FSFileReader, dir_manager: D) -> Self {
        Self {
            file_writer,
            file_reader,
            dir_manager,
        }
    }
}

impl<D> Storer<D>
where
    D: DirectoryManager,
{
    // TODO: For when we address the DirectoryManager dep injection
    // pub fn new() -> Self {
    //     Self {
    //         file_writer: WriterFile::default(),
    //         dir_manager: DirectoryManagerFs::default(),
    //     }
    // }
    fn write_contents(&self, agent_id: &AgentID, ds: &DataStored) -> Result<(), StorerError> {
        let dest_file = get_uild_path(agent_id);
        // Get a ref to the target file's parent directory
        let dest_dir = dest_file
            .parent()
            .expect("no parent directory found for {dest_file} (empty or root dir)");

        self.dir_manager
            .create(dest_dir, Permissions::from_mode(DIRECTORY_PERMISSIONS))?;
        let contents = serde_yaml::to_string(ds)?;

        Ok(self.file_writer.write(
            &dest_file,
            contents,
            Permissions::from_mode(FILE_PERMISSIONS),
        )?)
    }

    fn read_contents(&self, agent_id: &AgentID) -> Result<Option<DataStored>, StorerError> {
        let dest_path = get_uild_path(agent_id);
        let file_str = match self.file_reader.read(dest_path.as_path()) {
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
    use crate::config::super_agent_configs::AgentID;
    use crate::fs::directory_manager::test::MockDirectoryManagerMock;
    use crate::fs::file_reader::MockFSFileReader;
    use crate::fs::writer_file::MockWriterFile;
    use crate::opamp::instance_id::getter::DataStored;
    use crate::opamp::instance_id::on_host::storer::get_uild_path;
    use crate::opamp::instance_id::storer::InstanceIDStorer;
    use crate::opamp::instance_id::{Identifiers, InstanceID, Storer};
    use crate::super_agent::defaults::{REMOTE_AGENT_DATA_DIR, SUPER_AGENT_IDENTIFIERS_PATH};
    use mockall::predicate;
    use std::fs::Permissions;
    use std::io::{self, ErrorKind};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    #[test]
    fn basic_get_uild_path() {
        let agent_id = AgentID::new("test").unwrap();
        let path = get_uild_path(&agent_id);
        assert_eq!(
            path,
            PathBuf::from(format!("{}/test/identifiers.yaml", REMOTE_AGENT_DATA_DIR))
        );

        let super_agent_id = AgentID::new_super_agent_id();
        let path = get_uild_path(&super_agent_id);
        assert_eq!(path, PathBuf::from(SUPER_AGENT_IDENTIFIERS_PATH));
    }

    #[test]
    fn test_successful_write() {
        // Data
        let agent_id = AgentID::new("test").unwrap();
        let mut file_writer = MockWriterFile::default();
        let mut dir_manager = MockDirectoryManagerMock::default();
        let file_reader = MockFSFileReader::new();
        let ds = DataStored {
            ulid: InstanceID::new("test-ULID".to_owned()),
            identifiers: Identifiers {
                hostname: "test-hostname".to_string(),
                machine_id: "test-machine-id".to_string(),
                cloud_instance_id: "test-instance-id".to_string(),
            },
        };

        let ulid_path = get_uild_path(&agent_id);

        // Expectations
        dir_manager.should_create(ulid_path.parent().unwrap(), Permissions::from_mode(0o700));
        file_writer.should_write(
            &ulid_path,
            String::from("ulid: test-ULID\nidentifiers:\n  hostname: test-hostname\n  machine_id: test-machine-id\n  cloud_instance_id: test-instance-id\n"),
            Permissions::from_mode(0o600),
        );

        let storer = Storer::new(file_writer, file_reader, dir_manager);
        assert!(storer.set(&agent_id, &ds).is_ok());
    }

    #[test]
    fn test_unsuccessful_write() {
        // Data
        let agent_id = AgentID::new("test").unwrap();
        let mut file_writer = MockWriterFile::default();
        let mut dir_manager = MockDirectoryManagerMock::default();
        let file_reader = MockFSFileReader::new();
        let ds = DataStored {
            ulid: InstanceID::new("test-ULID".to_owned()),
            identifiers: Identifiers {
                hostname: "test-hostname".to_string(),
                machine_id: "test-machine-id".to_string(),
                cloud_instance_id: "test-instance-id".to_string(),
            },
        };

        let ulid_path = get_uild_path(&agent_id);

        // Expectations
        file_writer.should_not_write(
            &ulid_path,
            String::from("ulid: test-ULID\nidentifiers:\n  hostname: test-hostname\n  machine_id: test-machine-id\n  cloud_instance_id: test-instance-id\n"),
            Permissions::from_mode(0o600),
        );
        dir_manager.should_create(ulid_path.parent().unwrap(), Permissions::from_mode(0o700));

        let storer = Storer::new(file_writer, file_reader, dir_manager);
        assert!(storer.set(&agent_id, &ds).is_err());
    }

    #[test]
    fn test_successful_read() {
        // Data
        let agent_id = AgentID::new("test").unwrap();
        let file_writer = MockWriterFile::default();
        let dir_manager = MockDirectoryManagerMock::default();
        let mut file_reader = MockFSFileReader::new();
        let ds = DataStored {
            ulid: InstanceID::new("test-ULID".to_owned()),
            identifiers: Identifiers {
                hostname: "test-hostname".to_string(),
                machine_id: "test-machine-id".to_string(),
                cloud_instance_id: "test-instance-id".to_string(),
            },
        };
        let expected = Some(ds.clone());
        let ulid_path = get_uild_path(&agent_id);

        // Expectations
        file_reader
            .expect_read()
            .with(predicate::function(move |p| p == ulid_path.as_path()))
            .once()
            .return_once(|_| Ok(String::from("ulid: test-ULID\nidentifiers:\n  hostname: test-hostname\n  machine_id: test-machine-id\n  cloud_instance_id: test-instance-id\n")));

        let storer = Storer::new(file_writer, file_reader, dir_manager);
        let actual = storer.get(&agent_id);
        assert!(actual.is_ok());
        assert_eq!(expected, actual.unwrap());
    }

    #[test]
    fn test_unsuccessful_read() {
        let agent_id = AgentID::new("test").unwrap();
        let file_writer = MockWriterFile::default();
        let dir_manager = MockDirectoryManagerMock::default();
        let mut file_reader = MockFSFileReader::new();
        let ulid_path = get_uild_path(&agent_id);

        file_reader
            .expect_read()
            .with(predicate::function(move |p| p == ulid_path.as_path()))
            .once()
            .return_once(|_| Err(io::Error::new(ErrorKind::Other, "some error message").into()));

        let storer = Storer::new(file_writer, file_reader, dir_manager);
        let expected = storer.get(&agent_id);

        // As said above, we are not generatinc the error variant here
        assert!(expected.is_ok());
        assert!(expected.unwrap().is_none());
    }
}
