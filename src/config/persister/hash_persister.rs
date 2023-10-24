use thiserror::Error;

use crate::config::persister::config_writer_file::WriteError;
use crate::config::super_agent_configs::AgentID;

#[derive(Error, Debug)]
pub enum PersistError {
    #[error("file error: `{0}`")]
    FileError(#[from] WriteError),
}

pub trait HashPersister {
    fn persist(&self, agent_id: &AgentID, hash: String) -> Result<(), PersistError>;
}

////////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
pub mod test {
    use crate::config::persister::config_writer_file::WriteError;
    use crate::config::persister::config_writer_file::WriteError::InvalidPath;
    use crate::config::persister::fs_utils::FsError;
    use crate::config::super_agent_configs::AgentID;
    use mockall::mock;
    use std::io::{Error, ErrorKind};

    use super::{HashPersister, PersistError};

    impl Clone for PersistError {
        fn clone(&self) -> Self {
            match self {
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
    }

    mock! {
        pub(crate) HashPersisterMock {}

        impl HashPersister for HashPersisterMock {
             fn persist(&self,  agent_id: &AgentID, hash: String) -> Result<(), PersistError>;
        }
    }

    impl MockHashPersisterMock {
        pub fn should_persist_any(&mut self, times: usize) {
            self.expect_persist().times(times).returning(|_, _| Ok(()));
        }

        pub fn should_not_persist_any(&mut self, err: PersistError) {
            self.expect_persist()
                .once()
                .returning(move |_, _| Err(err.clone()));
        }
    }
}
