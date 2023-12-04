use std::fs::{self, File, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::config::persister::config_writer_file::WriteError;
#[cfg_attr(test, mockall_double::double)]
use crate::config::persister::config_writer_file::WriterFile;

use crate::config::persister::directory_manager::{
    DirectoryManagementError, DirectoryManager, DirectoryManagerFs,
};
use crate::config::super_agent_configs::AgentID;
use crate::opamp::instance_id::getter::{DataStored, InstanceID};
use crate::opamp::instance_id::storer::InstanceIDStorer;
use crate::opamp::instance_id::Identifiers;
use crate::super_agent::defaults::{IDENTIFIERS_DIR, SUPER_AGENT_IDENTIFIERS_PATH};
use tracing::debug;
use ulid::Ulid;

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
    dir_manager: D,
}

#[derive(thiserror::Error, Debug)]
pub enum StorerError {
    #[error("Generic error")]
    Generic,
    #[error("Error deserializing into an identifiers file:`{0}`")]
    Serialization(#[from] serde_yaml::Error),
    #[error("Directory management error: `{0}`")]
    DirectoryManagement(#[from] DirectoryManagementError),
    #[error("error writing file: `{0}`")]
    WriteError(#[from] WriteError),
    #[error("error creating file: `{0}`")]
    IOError(#[from] io::Error),
}

fn get_uild_path(agent_id: &AgentID) -> PathBuf {
    if agent_id.is_super_agent_id() {
        PathBuf::from(SUPER_AGENT_IDENTIFIERS_PATH)
    } else {
        PathBuf::from(format!("{}/{}.yaml", IDENTIFIERS_DIR, agent_id.get()))
    }
}

impl InstanceIDStorer for Storer {
    fn set(&self, agent_id: &AgentID, ds: &DataStored) -> Result<(), StorerError> {
        self.write_contents(agent_id, ds)
    }

    /// TODO
    fn get(&self, agent_id: &AgentID) -> Result<Option<DataStored>, StorerError> {
        self.read_contents(agent_id)
    }
}
impl Storer {
    fn write_contents(&self, agent_id: &AgentID, ds: &DataStored) -> Result<(), StorerError> {
        self.dir_manager.create(
            Path::new(IDENTIFIERS_DIR),
            Permissions::from_mode(DIRECTORY_PERMISSIONS),
        )?;
        let dest_path = get_uild_path(agent_id);
        let contents = serde_yaml::to_string(ds)?;

        Ok(self.file_writer.write(
            dest_path.as_path(),
            contents,
            Permissions::from_mode(FILE_PERMISSIONS),
        )?)
    }

    fn read_contents(&self, agent_id: &AgentID) -> Result<Option<DataStored>, StorerError> {
        let dest_path = get_uild_path(agent_id);
        if !dest_path.exists() {
            return Ok(None);
        }

        let file = match File::open(dest_path) {
            Ok(file) => file,
            Err(e) => {
                debug!("Could not open file: {}", e);
                return Ok(None);
            }
        };

        match serde_yaml::from_reader(file) {
            Ok(ds) => Ok(Some(ds)),
            Err(e) => {
                debug!("Could not deserialize file: {}", e);
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::config::persister::config_writer_file::{MockWriterFile, WriteError};
    use crate::config::persister::directory_manager::test::MockDirectoryManagerMock;
    use crate::config::super_agent_configs::AgentID;
    use crate::opamp::instance_id::getter::DataStored;
    use crate::opamp::instance_id::on_host::storer::get_uild_path;
    use crate::opamp::instance_id::{Storer, StorerError};
    use crate::super_agent::defaults::{IDENTIFIERS_DIR, SUPER_AGENT_IDENTIFIERS_PATH};
    use mockall::predicate;
    use nix::libc::pathconf;
    use std::fs::Permissions;
    use std::io::{self, ErrorKind};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    #[test]
    fn basic_get_uild_path() {
        let agent_id = AgentID::new("test").unwrap();
        let path = get_uild_path(&agent_id);
        assert_eq!(
            path,
            PathBuf::from(format!("{}/test.yaml", IDENTIFIERS_DIR))
        );

        let super_agent_id = AgentID::new_super_agent_id();
        let path = get_uild_path(&super_agent_id);
        assert_eq!(path, PathBuf::from(SUPER_AGENT_IDENTIFIERS_PATH));
    }

    #[test]
    fn test_successful_write() {
        let mut file_writer = MockWriterFile::default();
        file_writer.should_write(
            Path::new(""),
            String::default(),
            Permissions::from_mode(0o645),
        );

        let mut dir_manager = MockDirectoryManagerMock::default();
        dir_manager.should_create(Path::new(IDENTIFIERS_DIR), Permissions::from_mode(0o700));

        let storer = Storer {
            file_writer,
            dir_manager,
        };
    }
}
