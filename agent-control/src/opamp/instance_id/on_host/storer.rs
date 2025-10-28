use super::getter::Identifiers;
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::STORE_KEY_INSTANCE_ID;
use crate::on_host::file_store::FileStore;
use crate::opamp::instance_id::getter::DataStored;
use crate::opamp::instance_id::storer::InstanceIDStorer;
use fs::LocalFile;
use fs::directory_manager::{DirectoryManagementError, DirectoryManager, DirectoryManagerFs};
use fs::file_reader::{FileReader, FileReaderError};
use fs::writer_file::{FileWriter, WriteError};
use std::io;
use std::sync::Arc;
use tracing::debug;

pub struct Storer<F = LocalFile, D = DirectoryManagerFs>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    file_store: Arc<FileStore<F, D>>,
}

impl<F, D> From<Arc<FileStore<F, D>>> for Storer<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    fn from(file_store: Arc<FileStore<F, D>>) -> Self {
        Self { file_store }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum StorerError {
    #[error("generic error")]
    Generic,
    #[error("error (de)serializing from/into an identifiers file: {0}")]
    Serde(#[from] serde_yaml::Error),
    #[error("directory management error: {0}")]
    DirectoryManagement(#[from] DirectoryManagementError),
    #[error("error writing file: {0}")]
    WriteError(#[from] WriteError),
    #[error("error creating file: {0}")]
    IOError(#[from] io::Error),
    #[error("error reading file: {0}")]
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
        debug!("storer: setting Instance ID of agent_id: {}", agent_id);

        self.file_store
            .set_opamp_data(agent_id, STORE_KEY_INSTANCE_ID, ds)?;

        Ok(())
    }

    fn get(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<DataStored<Self::Identifiers>>, Self::Error> {
        debug!("storer: getting Instance ID of agent_id: {}", agent_id);

        if let Some(data) = self
            .file_store
            .get_opamp_data(agent_id, STORE_KEY_INSTANCE_ID)?
        {
            return Ok(Some(data));
        }

        Ok(None)
    }
}

// impl<F, D> Storer<F, D>
// where
//     D: DirectoryManager,
//     F: FileWriter + FileReader,
// {
//     pub fn new(file_rw: F, dir_manager: D, remote_dir: PathBuf) -> Self {
//         Self {
//             file_rw,
//             dir_manager,
//             remote_dir,
//         }
//     }
// }

// impl<F, D> Storer<F, D>
// where
//     D: DirectoryManager,
//     F: FileWriter + FileReader,
// {
//     fn write_contents(
//         &self,
//         agent_id: &AgentID,
//         ds: &DataStored<Identifiers>,
//     ) -> Result<(), StorerError> {
//         let dest_file = self.get_instance_id_path(agent_id);
//         // Get a ref to the target file's parent directory
//         let dest_dir = dest_file
//             .parent()
//             .ok_or(WriteError::from(FsError::InvalidPath(format!(
//                 "no parent directory found for {} (empty or root dir)",
//                 dest_file.display()
//             ))))?;

//         self.dir_manager.create(dest_dir)?;
//         let contents = serde_yaml::to_string(ds)?;

//         Ok(self.file_rw.write(&dest_file, contents)?)
//     }

//     fn read_contents(
//         &self,
//         agent_id: &AgentID,
//     ) -> Result<Option<DataStored<Identifiers>>, StorerError> {
//         let dest_path = self.get_instance_id_path(agent_id);
//         let file_str = match self.file_rw.read(dest_path.as_path()) {
//             Ok(s) => s,
//             Err(e) => {
//                 debug!("error reading file for agent {}: {}", agent_id, e);
//                 return Ok(None);
//             }
//         };
//         match serde_yaml::from_str(&file_str) {
//             Ok(ds) => Ok(Some(ds)),
//             Err(e) => {
//                 debug!("error deserializing data for agent {}: {}", agent_id, e);
//                 Ok(None)
//             }
//         }
//     }

//     fn get_instance_id_path(&self, agent_id: &AgentID) -> PathBuf {
//         self.remote_dir
//             .join(FOLDER_NAME_FLEET_DATA)
//             .join(agent_id)
//             .join(INSTANCE_ID_FILENAME)
//     }
// }

pub fn build_config_name(name: &str) -> String {
    format!("{name}.yaml")
}

#[cfg(test)]
mod tests {
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::defaults::{FOLDER_NAME_FLEET_DATA, INSTANCE_ID_FILENAME};
    use crate::on_host::file_store::FileStore;
    use crate::opamp::instance_id::InstanceID;
    use crate::opamp::instance_id::getter::DataStored;
    use crate::opamp::instance_id::on_host::getter::Identifiers;
    use crate::opamp::instance_id::on_host::storer::Storer;
    use crate::opamp::instance_id::storer::InstanceIDStorer;
    use fs::directory_manager::mock::MockDirectoryManager;
    use fs::mock::MockLocalFile;
    use mockall::predicate;
    use std::io;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn basic_get_uild_path() {
        let sa_dir = PathBuf::from("/super");
        let file_store = Arc::new(FileStore::new(
            MockLocalFile::default(),
            MockDirectoryManager::default(),
            PathBuf::default(),
            sa_dir.clone(),
        ));
        let storer = Storer::from(file_store);
        let agent_id = AgentID::try_from("test").unwrap();
        let path = storer.file_store.get_testing_instance_id_path(&agent_id);
        assert_eq!(
            path,
            sa_dir
                .join(FOLDER_NAME_FLEET_DATA)
                .join("test")
                .join(INSTANCE_ID_FILENAME)
        );

        let agent_control_id = AgentID::AgentControl;
        let path = storer
            .file_store
            .get_testing_instance_id_path(&agent_control_id);
        assert_eq!(
            path,
            sa_dir
                .join("fleet-data/agent-control")
                .join(INSTANCE_ID_FILENAME)
        );
    }

    #[test]
    fn test_successful_write() {
        // Data
        let (agent_id, sa_path, instance_id_path) = test_data();
        let mut file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManager::default();
        let instance_id = InstanceID::create();
        let ds = DataStored {
            instance_id: instance_id.clone(),
            identifiers: test_identifiers(),
        };

        // Expectations
        dir_manager.should_create(instance_id_path.parent().unwrap());
        file_rw.should_write(&instance_id_path, expected_file(instance_id));

        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            PathBuf::default(),
            sa_path,
        ));

        let storer = Storer::from(file_store);
        assert!(storer.set(&agent_id, &ds).is_ok());
    }

    #[test]
    fn test_unsuccessful_write() {
        // Data
        let (agent_id, sa_path, instance_id_path) = test_data();
        let mut file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManager::default();
        let instance_id = InstanceID::create();
        let ds = DataStored {
            instance_id: instance_id.clone(),
            identifiers: test_identifiers(),
        };

        // Expectations
        file_rw.should_not_write(&instance_id_path, expected_file(instance_id));
        dir_manager.should_create(instance_id_path.parent().unwrap());

        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            PathBuf::default(),
            sa_path.clone(),
        ));

        let storer = Storer::from(file_store);
        assert!(storer.set(&agent_id, &ds).is_err());
    }

    #[test]
    fn test_successful_read() {
        // Data
        let (agent_id, sa_path, instance_id_path) = test_data();
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

        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            PathBuf::default(),
            sa_path,
        ));
        let storer = Storer::from(file_store);
        let actual = storer.get(&agent_id);
        assert!(actual.is_ok());
        assert_eq!(expected, actual.unwrap());
    }

    #[test]
    fn test_unsuccessful_read() {
        let (agent_id, sa_path, instance_id_path) = test_data();
        let mut file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManager::default();

        file_rw
            .expect_read()
            .with(predicate::function(move |p| {
                p == instance_id_path.as_path()
            }))
            .once()
            .return_once(|_| Err(io::Error::other("some error message").into()));

        let file_store = Arc::new(FileStore::new(
            file_rw,
            dir_manager,
            PathBuf::default(),
            sa_path,
        ));
        let storer = Storer::from(file_store);
        let expected = storer.get(&agent_id);

        // As said above, we are not generating the error variant here
        assert!(
            matches!(expected, Err(ref s) if s.to_string().contains("some error message")),
            "Expected Err variant, got {:?}",
            expected
        );
    }

    // HELPERS

    const HOSTNAME: &str = "test-hostname";
    const MACHINE_ID: &str = "test-machine-id";
    const CLOUD_INSTANCE_ID: &str = "test-instance-id";
    const HOST_ID: &str = "test-host-id";
    const FLEET_ID: &str = "test-fleet-id";

    fn test_data() -> (AgentID, PathBuf, PathBuf) {
        let agent_id = AgentID::try_from("test").unwrap();
        let sa_path = PathBuf::from("/super");
        let instance_id_path = PathBuf::from("/super/fleet-data/test/instance_id.yaml");
        (agent_id, sa_path, instance_id_path)
    }

    fn expected_file(instance_id: InstanceID) -> String {
        format!(
            "instance_id: {instance_id}\nidentifiers:\n  hostname: {HOSTNAME}\n  machine_id: {MACHINE_ID}\n  cloud_instance_id: {CLOUD_INSTANCE_ID}\n  host_id: {HOST_ID}\n  fleet_id: {FLEET_ID}\n",
        )
    }

    fn test_identifiers() -> Identifiers {
        Identifiers {
            hostname: HOSTNAME.into(),
            machine_id: MACHINE_ID.into(),
            cloud_instance_id: CLOUD_INSTANCE_ID.into(),
            host_id: HOST_ID.into(),
            fleet_id: FLEET_ID.into(),
        }
    }
}
