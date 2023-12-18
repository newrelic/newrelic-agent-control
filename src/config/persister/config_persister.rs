use thiserror::Error;

use crate::config::agent_type::agent_types::FinalAgent;
use crate::config::persister::config_writer_file::WriteError;
use crate::config::persister::directory_manager::DirectoryManagementError;
use crate::config::super_agent_configs::AgentID;

#[derive(Error, Debug)]
pub enum PersistError {
    #[error("directory error: `{0}`")]
    DirectoryError(#[from] DirectoryManagementError),

    #[error("file error: `{0}`")]
    FileError(#[from] WriteError),
}

pub trait ConfigurationPersister {
    fn persist_agent_config(
        &self,
        agent_id: &AgentID,
        agent_type: &FinalAgent,
    ) -> Result<(), PersistError>;

    fn delete_agent_config(
        &self,
        agent_id: &AgentID,
        agent_type: &FinalAgent,
    ) -> Result<(), PersistError>;
    // clean all agents configurations
    fn delete_all_configs(&self) -> Result<(), PersistError>;
}

////////////////////////////////////////////////////////////////////////////////////
// TESTS
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
pub mod test {
    use crate::config::agent_type::agent_types::FinalAgent;
    use crate::config::persister::config_writer_file::WriteError;
    use crate::config::persister::config_writer_file::WriteError::InvalidPath;
    use crate::config::persister::directory_manager::DirectoryManagementError::{
        ErrorCreatingDirectory, ErrorDeletingDirectory, InvalidDirectory,
    };
    use crate::config::persister::fs_utils::FsError;
    use crate::config::super_agent_configs::AgentID;
    use mockall::{mock, predicate};
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
             fn persist_agent_config(&self, agent_id: &AgentID, agent_type: &FinalAgent) -> Result<(), PersistError>;
             fn delete_agent_config(&self, agent_id: &AgentID, agent_type: &FinalAgent) -> Result<(), PersistError>;
             fn delete_all_configs(&self) -> Result<(), PersistError>;
        }
    }

    impl MockConfigurationPersisterMock {
        pub fn should_persist_agent_config(&mut self, agent_id: &AgentID, agent_type: &FinalAgent) {
            self.expect_persist_agent_config()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(agent_type.clone()),
                )
                .returning(|_, _| Ok(()));
        }

        pub fn should_not_persist_agent_config(
            &mut self,
            agent_id: &AgentID,
            final_agent: &FinalAgent,
            err: PersistError,
        ) {
            self.expect_persist_agent_config()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(final_agent.clone()),
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
        pub fn should_delete_agent_config(&mut self, agent_id: &AgentID, final_agent: &FinalAgent) {
            self.expect_delete_agent_config()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(final_agent.clone()),
                )
                .returning(|_, _| Ok(()));
        }

        pub fn should_not_delete_agent_config(
            &mut self,
            agent_id: &AgentID,
            final_agent: &FinalAgent,
            err: PersistError,
        ) {
            self.expect_delete_agent_config()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(final_agent.clone()),
                )
                .returning(move |_, _| Err(err.clone()));
        }

        #[allow(dead_code)]
        pub fn should_delete_any_agent_config(&mut self, times: usize) {
            self.expect_delete_agent_config()
                .times(times)
                .returning(|_, _| Ok(()));
        }

        // cannot assert on what is cleaned because of hashmap order
        #[allow(dead_code)]
        pub fn should_not_delete_any_agent_config(&mut self, err: PersistError) {
            self.expect_delete_agent_config()
                .once()
                .returning(move |_, _| Err(err.clone()));
        }

        #[allow(dead_code)]
        pub fn should_delete_all_configs(&mut self) {
            self.expect_delete_all_configs().once().returning(|| Ok(()));
        }

        #[allow(dead_code)]
        pub fn should_delete_all_configs_times(&mut self, times: usize) {
            self.expect_delete_all_configs()
                .times(times)
                .returning(|| Ok(()));
        }
    }
}
