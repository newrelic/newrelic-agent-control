use thiserror::Error;

use crate::config::agent_configs::AgentID;
use crate::config::agent_type::Agent as AgentType;
use crate::config::persister::config_writer_file::WriteError;
use crate::config::persister::directory_manager::DirectoryManagementError;

#[derive(Error, Debug)]
pub enum PersistError {
    #[error("directory error: `{0}`")]
    DirectoryError(#[from] DirectoryManagementError),

    #[error("file error: `{0}`")]
    FileError(#[from] WriteError),
}

pub trait ConfigurationPersister {
    fn persist(&self, agent_id: &AgentID, agent_type: &AgentType) -> Result<(), PersistError>;

    // TODO not sure if agent_type is/will be needed here
    fn clean(&self, agent_id: &AgentID, agent_type: &AgentType) -> Result<(), PersistError>;
}

////////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
pub mod test {
    use crate::config::agent_configs::AgentID;
    use crate::config::agent_type::Agent as AgentType;
    use crate::config::persister::config_writer_file::WriteError;
    use crate::config::persister::config_writer_file::WriteError::InvalidPath;
    use crate::config::persister::directory_manager::DirectoryManagementError::{
        ErrorCreatingDirectory, ErrorDeletingDirectory, InvalidDirectory,
    };
    use crate::config::persister::fs_utils::FsError;
    use mockall::mock;
    use std::io::{Error, ErrorKind};

    use super::{ConfigurationPersister, PersistError};

    impl Clone for PersistError {
        fn clone(&self) -> Self {
            match self {
                PersistError::DirectoryError(dir_error) => match dir_error {
                    ErrorCreatingDirectory(path, create_error) => PersistError::DirectoryError(
                        ErrorCreatingDirectory(path.to_string(), create_error.to_string()),
                    ),
                    ErrorDeletingDirectory(delete_error) => PersistError::DirectoryError(
                        ErrorDeletingDirectory(delete_error.to_string()),
                    ),
                    InvalidDirectory(invalid_dir_error) => match invalid_dir_error {
                        FsError::InvalidPath() => {
                            PersistError::DirectoryError(InvalidDirectory(FsError::InvalidPath()))
                        }
                        FsError::DotsDisallowed(msg) => PersistError::DirectoryError(
                            InvalidDirectory(FsError::DotsDisallowed(msg.to_string())),
                        ),
                    },
                },
                PersistError::FileError(write_error) => {
                    match write_error {
                        WriteError::ErrorCreatingFile(_) => {
                            // we hardcode this one for simplicity
                            PersistError::FileError(WriteError::ErrorCreatingFile(Error::from(
                                ErrorKind::PermissionDenied,
                            )))
                        }
                        InvalidPath(fs_error) => match fs_error {
                            FsError::InvalidPath() => {
                                PersistError::FileError(InvalidPath(FsError::InvalidPath()))
                            }
                            FsError::DotsDisallowed(path) => PersistError::FileError(InvalidPath(
                                FsError::DotsDisallowed(path.to_string()),
                            )),
                        },
                    }
                }
            }
        }

        fn clone_from(&mut self, _: &Self) {
            unimplemented!()
        }
    }

    mock! {
        pub(crate) ConfigurationPersisterMock {}

        impl ConfigurationPersister for ConfigurationPersisterMock {
             fn persist(&self, agent_id: &AgentID, agent_type: &AgentType) -> Result<(), PersistError>;
             fn clean(&self, agent_id: &AgentID, agent_type: &AgentType) -> Result<(), PersistError>;
        }
    }

    impl MockConfigurationPersisterMock {
        pub fn should_persist_any(&mut self, times: usize) {
            self.expect_persist().times(times).returning(|_, _| Ok(()));
        }

        pub fn should_not_persist_any(&mut self, err: PersistError) {
            self.expect_persist()
                .once()
                .returning(move |_, _| Err(err.clone()));
        }
        pub fn should_clean_any(&mut self, times: usize) {
            self.expect_clean().times(times).returning(|_, _| Ok(()));
        }

        // cannot assert on what is cleaned because of hashmap order
        pub fn should_not_clean_any(&mut self, err: PersistError) {
            self.expect_clean()
                .once()
                .returning(move |_, _| Err(err.clone()));
        }
    }
}
