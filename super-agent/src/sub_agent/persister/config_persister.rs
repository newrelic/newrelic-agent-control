use std::collections::HashMap;

use thiserror::Error;

use crate::agent_type::variable::definition::VariableDefinition;
use crate::super_agent::config::AgentID;
use fs::directory_manager::DirectoryManagementError;
use fs::writer_file::WriteError;

#[derive(Error, Debug)]
pub enum PersistError {
    #[error("directory error: `{0}`")]
    DirectoryError(#[from] DirectoryManagementError),

    #[error("file error: `{0}`")]
    FileError(#[from] WriteError),
}

/// ConfigurationPersister defines the functions to persist and delete the values provided in `variables` whose
/// kind requires persistence.
pub trait ConfigurationPersister {
    fn persist_agent_config(
        &self,
        agent_id: &AgentID,
        variables: &HashMap<String, VariableDefinition>,
    ) -> Result<(), PersistError>;

    fn delete_agent_config(&self, agent_id: &AgentID) -> Result<(), PersistError>;
}

////////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
pub mod test {
    use crate::agent_type::variable::definition::VariableDefinition;
    use crate::super_agent::config::AgentID;
    use fs::directory_manager::DirectoryManagementError::{
        ErrorCreatingDirectory, ErrorDeletingDirectory, InvalidDirectory,
    };
    use fs::utils::FsError;
    use fs::writer_file::WriteError;
    use fs::writer_file::WriteError::InvalidPath;
    use mockall::{mock, predicate};
    use std::collections::HashMap;
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
                        WriteError::DirectoryError(_) => {
                            // we hardcode this one for simplicity
                            PersistError::DirectoryError(ErrorDeletingDirectory(
                                "oh no...".to_string(),
                            ))
                        }
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
             fn persist_agent_config(&self, agent_id: &AgentID, variables: &HashMap<String, VariableDefinition>) -> Result<(), PersistError>;
             fn delete_agent_config(&self, agent_id: &AgentID) -> Result<(), PersistError>;
        }
    }

    impl MockConfigurationPersisterMock {
        pub fn should_persist_agent_config(
            &mut self,
            agent_id: &AgentID,
            variables: &HashMap<String, VariableDefinition>,
        ) {
            self.expect_persist_agent_config()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(variables.clone()),
                )
                .returning(|_, _| Ok(()));
        }

        pub fn should_not_persist_agent_config(
            &mut self,
            agent_id: &AgentID,
            variables: &HashMap<String, VariableDefinition>,
            err: PersistError,
        ) {
            self.expect_persist_agent_config()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(variables.clone()),
                )
                .once()
                .returning(move |_, _| Err(err.clone()));
        }

        #[allow(dead_code)]
        pub fn should_persist_any_agent_config(&mut self, times: usize) {
            self.expect_persist_agent_config()
                .times(times)
                .returning(|_, _| Ok(()));
        }

        #[allow(dead_code)]
        pub fn should_not_persist_any_agent_config(&mut self, err: PersistError) {
            self.expect_persist_agent_config()
                .once()
                .returning(move |_, _| Err(err.clone()));
        }
        pub fn should_delete_agent_config(
            &mut self,
            agent_id: &AgentID,
            variables: &HashMap<String, VariableDefinition>,
        ) {
            self.expect_delete_agent_config()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .returning(|_| Ok(()));
        }

        pub fn should_not_delete_agent_config(
            &mut self,
            agent_id: &AgentID,
            variables: &HashMap<String, VariableDefinition>,
            err: PersistError,
        ) {
            self.expect_delete_agent_config()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .returning(move |_| Err(err.clone()));
        }

        #[allow(dead_code)]
        pub fn should_delete_any_agent_config(&mut self, times: usize) {
            self.expect_delete_agent_config()
                .times(times)
                .returning(|_| Ok(()));
        }

        // cannot assert on what is cleaned because of hashmap order
        #[allow(dead_code)]
        pub fn should_not_delete_any_agent_config(&mut self, err: PersistError) {
            self.expect_delete_agent_config()
                .once()
                .returning(move |_| Err(err.clone()));
        }
    }
}
