use crate::agent_type::definition::AgentType;
use crate::super_agent::config::AgentID;
use crate::super_agent::defaults::{VALUES_DIR, VALUES_FILE};
use crate::values::yaml_config::YAMLConfig;
use crate::values::yaml_config_repository::{YAMLConfigRepository, YAMLConfigRepositoryError};
use fs::directory_manager::{DirectoryManagementError, DirectoryManager, DirectoryManagerFs};
use fs::file_reader::{FileReader, FileReaderError};
use fs::writer_file::{FileWriter, WriteError};
use fs::LocalFile;
use std::fs::Permissions;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use thiserror::Error;
use tracing::error;

#[cfg(target_family = "unix")]
pub const FILE_PERMISSIONS: u32 = 0o600;
#[cfg(target_family = "unix")]
const DIRECTORY_PERMISSIONS: u32 = 0o700;

#[derive(Error, Debug)]
pub enum OnHostYAMLConfigRepositoryError {
    #[error("serialize error loading SubAgentConfig: `{0}`")]
    StoreSerializeError(#[from] serde_yaml::Error),
    #[error("directory manager error: `{0}`")]
    DirectoryManagementError(#[from] DirectoryManagementError),
    #[error("file write error: `{0}`")]
    WriteError(#[from] WriteError),
    #[error("file read error: `{0}`")]
    ReadError(#[from] FileReaderError),
    #[cfg(test)]
    #[error("common variant for k8s and on-host implementations")]
    Generic,
}

pub struct YAMLConfigRepositoryFile<F, S>
where
    S: DirectoryManager,
    F: FileWriter + FileReader,
{
    directory_manager: S,
    file_rw: F,
    remote_conf_path: PathBuf,
    local_conf_path: PathBuf,
    remote_enabled: bool,
}

impl YAMLConfigRepositoryFile<LocalFile, DirectoryManagerFs> {
    pub fn new(remote_conf_path: PathBuf, local_conf_path: PathBuf) -> Self {
        YAMLConfigRepositoryFile {
            directory_manager: DirectoryManagerFs {},
            file_rw: LocalFile,
            remote_conf_path,
            local_conf_path,
            remote_enabled: false,
        }
    }

    pub fn with_remote(mut self) -> Self {
        self.remote_enabled = true;
        self
    }
}

impl<F, S> YAMLConfigRepositoryFile<F, S>
where
    S: DirectoryManager,
    F: FileWriter + FileReader,
{
    pub fn get_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        self.local_conf_path
            .join(agent_id)
            .join(VALUES_DIR())
            .join(VALUES_FILE())
    }

    pub fn get_remote_values_file_path(&self, agent_id: &AgentID) -> PathBuf {
        // This file (soon files) will often be removed, but its parent directory contains files
        // that should persist across these deletions. As opposed to its non-remote counterpart in
        // `get_values_file_path`, we put the values file inside its own directory, which will
        // be recreated each time a remote config is received, leaving the other files untouched.
        self.remote_conf_path
            .join(agent_id)
            .join(VALUES_DIR())
            .join(VALUES_FILE())
    }

    // Load a file contents only if the file is present.
    // If the file is not present there is no error nor file
    fn load_file_if_present(
        &self,
        path: PathBuf,
    ) -> Result<Option<YAMLConfig>, OnHostYAMLConfigRepositoryError> {
        let values_result = self.file_rw.read(path.as_path());
        match values_result {
            Err(FileReaderError::FileNotFound(_)) => {
                //actively fallback to load local file
                Ok(None)
            }
            Ok(res) => Ok(Some(serde_yaml::from_str(&res)?)),
            Err(err) => {
                // we log any unexpected error for now but maybe we should propagate it
                error!("error loading remote file {}", path.display());
                Err(err.into())
            }
        }
    }

    /// ensures directory exist and its empty
    fn ensure_directory_existence(
        &self,
        values_file_path: &PathBuf,
    ) -> Result<(), OnHostYAMLConfigRepositoryError> {
        let mut values_dir_path = PathBuf::from(&values_file_path);
        values_dir_path.pop();

        self.directory_manager.delete(values_dir_path.as_path())?;
        self.directory_manager.create(
            values_dir_path.as_path(),
            Permissions::from_mode(DIRECTORY_PERMISSIONS),
        )?;
        Ok(())
    }
}

impl<F, S> YAMLConfigRepository for YAMLConfigRepositoryFile<F, S>
where
    S: DirectoryManager + Send + Sync + 'static,
    F: FileWriter + FileReader + Send + Sync + 'static,
{
    fn load_local(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<YAMLConfig>, YAMLConfigRepositoryError> {
        let local_values_path = self.get_values_file_path(agent_id);
        self.load_file_if_present(local_values_path)
            .map_err(|err| YAMLConfigRepositoryError::LoadError(err.to_string()))
    }

    fn load_remote(
        &self,
        agent_id: &AgentID,
        agent_type: &AgentType,
    ) -> Result<Option<YAMLConfig>, YAMLConfigRepositoryError> {
        if !self.remote_enabled || !agent_type.has_remote_management() {
            return Ok(None);
        }

        let remote_values_path = self.get_remote_values_file_path(agent_id);
        self.load_file_if_present(remote_values_path)
            .map_err(|err| YAMLConfigRepositoryError::LoadError(err.to_string()))
    }

    fn store_remote(
        &self,
        agent_id: &AgentID,
        yaml_config: &YAMLConfig,
    ) -> Result<(), YAMLConfigRepositoryError> {
        let values_file_path = self.get_remote_values_file_path(agent_id);

        self.ensure_directory_existence(&values_file_path)
            .map_err(|err| YAMLConfigRepositoryError::StoreError(err.to_string()))?;

        let content = serde_yaml::to_string(yaml_config)
            .map_err(|err| YAMLConfigRepositoryError::StoreError(err.to_string()))?;

        self.file_rw
            .write(
                values_file_path.clone().as_path(),
                content,
                Permissions::from_mode(FILE_PERMISSIONS),
            )
            .map_err(|err| YAMLConfigRepositoryError::StoreError(err.to_string()))?;

        Ok(())
    }

    fn delete_remote(&self, agent_id: &AgentID) -> Result<(), YAMLConfigRepositoryError> {
        let values_file_path = self.get_remote_values_file_path(agent_id);
        //ensure directory exists
        let mut values_dir_path = values_file_path.clone();
        values_dir_path.pop();
        let values_dir = values_dir_path.to_str().unwrap().to_string();
        self.directory_manager
            .delete(values_dir_path.as_path())
            .map_err(|err| {
                YAMLConfigRepositoryError::DeleteError(format!(
                    "cannot delete path `{}`: `{}`",
                    values_dir, err
                ))
            })
    }
}

#[cfg(test)]
pub mod test {
    use super::YAMLConfigRepositoryFile;
    use crate::agent_type::definition::AgentType;
    use crate::super_agent::config::AgentID;
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::{YAMLConfigRepository, YAMLConfigRepositoryError};
    use assert_matches::assert_matches;
    use fs::directory_manager::mock::MockDirectoryManagerMock;
    use fs::directory_manager::DirectoryManagementError::{
        ErrorCreatingDirectory, ErrorDeletingDirectory,
    };
    use fs::directory_manager::DirectoryManager;
    use fs::file_reader::FileReader;
    use fs::mock::MockLocalFile;
    use fs::writer_file::FileWriter;
    use serde_yaml::Value;
    use std::collections::HashMap;
    use std::fs::Permissions;
    use std::path::Path;

    use crate::agent_type::environment::Environment;
    use crate::super_agent::defaults::default_capabilities;
    #[cfg(target_family = "unix")]
    use std::os::unix::fs::PermissionsExt;

    impl<F, S> YAMLConfigRepositoryFile<F, S>
    where
        S: DirectoryManager,
        F: FileWriter + FileReader,
    {
        pub fn with_mocks(
            file_rw: F,
            directory_manager: S,
            local_conf_path: &Path,
            remote_conf_path: &Path,
            remote_enabled: bool,
        ) -> Self {
            YAMLConfigRepositoryFile {
                file_rw,
                directory_manager,
                remote_conf_path: remote_conf_path.to_path_buf(),
                local_conf_path: local_conf_path.to_path_buf(),
                remote_enabled,
            }
        }
    }

    const SIMPLE_AGENT_TYPE: &str = r#"
namespace: newrelic
name: first
version: 0.1.0
variables:
  common:
    config_path:
      description: "config file string"
      type: string
      required: true
    config_argument:
      description: "config argument"
      type: string
      required: false
      default: bar
deployment:
  on_host:
    executables:
      - path: /opt/first
        args: "--config_path=${nr-var:config_path} --foo=${nr-var:config_argument}"
        env: ""
"#;

    #[test]
    fn test_load_when_remote_enabled() {
        //Mocks
        let mut file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = true;

        let agent_id = AgentID::new("some-agent-id").unwrap();
        let mut final_agent = AgentType::build_for_testing(SIMPLE_AGENT_TYPE, &Environment::OnHost);
        final_agent.set_capabilities(default_capabilities());

        let yaml_config_content = "some_config: true\nanother_item: false";

        file_rw.should_read(
            Path::new("some/remote/path/some-agent-id/values/values.yaml"),
            yaml_config_content.to_string(),
        );

        let repo = YAMLConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let yaml_config = repo.load(&agent_id, &final_agent).unwrap();

        assert_eq!(yaml_config.get("some_config").unwrap(), &Value::Bool(true));
        assert_eq!(
            yaml_config.get("another_item").unwrap(),
            &Value::Bool(false)
        );
    }

    #[test]
    fn test_load_when_remote_disabled() {
        //Mocks
        let mut file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some-agent-id").unwrap();
        let mut final_agent = AgentType::build_for_testing(SIMPLE_AGENT_TYPE, &Environment::OnHost);
        final_agent.set_capabilities(default_capabilities());

        let yaml_config_content = "some_config: true\nanother_item: false";

        file_rw.should_read(
            Path::new("some/local/path/some-agent-id/values/values.yaml"),
            yaml_config_content.to_string(),
        );

        let repo = YAMLConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let yaml_config = repo.load(&agent_id, &final_agent).unwrap();

        assert_eq!(yaml_config.get("some_config").unwrap(), &Value::Bool(true));
        assert_eq!(
            yaml_config.get("another_item").unwrap(),
            &Value::Bool(false)
        );
    }

    #[test]
    fn test_load_when_remote_enabled_file_not_found_fallbacks_to_local() {
        //Mocks
        let mut file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = true;

        let agent_id = AgentID::new("some-agent-id").unwrap();
        let mut final_agent = AgentType::build_for_testing(SIMPLE_AGENT_TYPE, &Environment::OnHost);
        final_agent.set_capabilities(default_capabilities());

        let yaml_config_content = "some_config: true\nanother_item: false";

        file_rw.should_not_read_file_not_found(
            Path::new("some/remote/path/some-agent-id/values/values.yaml"),
            "some_error_message".to_string(),
        );

        file_rw.should_read(
            Path::new("some/local/path/some-agent-id/values/values.yaml"),
            yaml_config_content.to_string(),
        );

        let repo = YAMLConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let yaml_config = repo.load(&agent_id, &final_agent).unwrap();

        assert_eq!(yaml_config.get("some_config").unwrap(), &Value::Bool(true));
        assert_eq!(
            yaml_config.get("another_item").unwrap(),
            &Value::Bool(false)
        );
    }

    #[test]
    fn test_load_local_file_not_found_should_return_defaults() {
        //Mocks
        let mut file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some-agent-id").unwrap();
        let mut final_agent = AgentType::build_for_testing(SIMPLE_AGENT_TYPE, &Environment::OnHost);
        final_agent.set_capabilities(default_capabilities());

        file_rw.should_not_read_file_not_found(
            Path::new("some/local/path/some-agent-id/values/values.yaml"),
            "some message".to_string(),
        );

        let repo = YAMLConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let yaml_config = repo.load(&agent_id, &final_agent).unwrap();

        assert_eq!(yaml_config, YAMLConfig::default());
    }

    #[test]
    fn test_load_when_remote_enabled_io_error() {
        //Mocks
        let mut file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = true;

        let agent_id = AgentID::new("some-agent-id").unwrap();
        let mut final_agent = AgentType::build_for_testing(SIMPLE_AGENT_TYPE, &Environment::OnHost);
        final_agent.set_capabilities(default_capabilities());

        file_rw.should_not_read_io_error(Path::new(
            "some/remote/path/some-agent-id/values/values.yaml",
        ));

        let repo = YAMLConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let result = repo.load(&agent_id, &final_agent);
        let err = result.unwrap_err();
        assert_matches!(err, YAMLConfigRepositoryError::LoadError(s) => {
            assert!(s.contains("file read error"));
        });
    }

    #[test]
    fn test_load_local_io_error() {
        //Mocks
        let mut file_rw = MockLocalFile::default();
        let dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some-agent-id").unwrap();
        let mut final_agent = AgentType::build_for_testing(SIMPLE_AGENT_TYPE, &Environment::OnHost);
        final_agent.set_capabilities(default_capabilities());

        file_rw.should_not_read_io_error(Path::new(
            "some/local/path/some-agent-id/values/values.yaml",
        ));

        let repo = YAMLConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let result = repo.load(&agent_id, &final_agent);
        let err = result.unwrap_err();
        assert_matches!(err, YAMLConfigRepositoryError::LoadError(s) => {
            assert!(s.contains("error reading contents"));
        });
    }

    #[test]
    fn test_store_remote() {
        //Mocks
        let mut file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some-agent-id").unwrap();
        let yaml_config = YAMLConfig::new(HashMap::from([("one_item".into(), "one value".into())]));

        dir_manager.should_delete(Path::new("some/remote/path/some-agent-id/values"));
        dir_manager.should_create(
            Path::new("some/remote/path/some-agent-id/values"),
            Permissions::from_mode(0o700),
        );

        file_rw.should_write(
            Path::new("some/remote/path/some-agent-id/values/values.yaml"),
            "one_item: one value\n".to_string(),
            Permissions::from_mode(0o600),
        );

        let repo = YAMLConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        repo.store_remote(&agent_id, &yaml_config).unwrap();
    }

    #[test]
    fn test_store_remote_error_deleting_dir() {
        //Mocks
        let file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some-agent-id").unwrap();
        let yaml_config = YAMLConfig::new(HashMap::from([("one_item".into(), "one value".into())]));

        dir_manager.should_not_delete(
            Path::new("some/remote/path/some-agent-id/values"),
            ErrorDeletingDirectory("oh now...".to_string()),
        );

        let repo = YAMLConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let result = repo.store_remote(&agent_id, &yaml_config);
        let err = result.unwrap_err();
        assert_matches!(err, YAMLConfigRepositoryError::StoreError(s) => {
            assert!(s.contains("cannot delete directory"));
        });
    }

    #[test]
    fn test_store_remote_error_creating_dir() {
        //Mocks
        let file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some-agent-id").unwrap();
        let yaml_config = YAMLConfig::new(HashMap::from([("one_item".into(), "one value".into())]));

        dir_manager.should_delete(Path::new("some/remote/path/some-agent-id/values"));
        dir_manager.should_not_create(
            Path::new("some/remote/path/some-agent-id/values"),
            Permissions::from_mode(0o700),
            ErrorCreatingDirectory("dir name".to_string(), "oh now...".to_string()),
        );

        let repo = YAMLConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let result = repo.store_remote(&agent_id, &yaml_config);
        let err = result.unwrap_err();
        assert_matches!(err, YAMLConfigRepositoryError::StoreError(s) => {
            assert!(s.contains("cannot create directory"));
        });
    }

    #[test]
    fn test_store_remote_error_writing_file() {
        //Mocks
        let mut file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some-agent-id").unwrap();
        let yaml_config = YAMLConfig::new(HashMap::from([("one_item".into(), "one value".into())]));

        dir_manager.should_delete(Path::new("some/remote/path/some-agent-id/values"));
        dir_manager.should_create(
            Path::new("some/remote/path/some-agent-id/values"),
            Permissions::from_mode(0o700),
        );

        file_rw.should_not_write(
            Path::new("some/remote/path/some-agent-id/values/values.yaml"),
            "one_item: one value\n".to_string(),
            Permissions::from_mode(0o600),
        );

        let repo = YAMLConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        let result = repo.store_remote(&agent_id, &yaml_config);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_matches!(err, YAMLConfigRepositoryError::StoreError(s) => {
            assert!(s.contains("error creating file"));
        });
    }

    #[test]
    fn test_delete_remote() {
        //Mocks
        let file_rw = MockLocalFile::default();
        let mut dir_manager = MockDirectoryManagerMock::new();
        let remote_conf_path = Path::new("some/remote/path");
        let local_conf_path = Path::new("some/local/path");
        let remote_enabled = false;

        let agent_id = AgentID::new("some-agent-id").unwrap();

        dir_manager.should_delete(Path::new("some/remote/path/some-agent-id/values"));

        let repo = YAMLConfigRepositoryFile::with_mocks(
            file_rw,
            dir_manager,
            local_conf_path,
            remote_conf_path,
            remote_enabled,
        );

        repo.delete_remote(&agent_id).unwrap();
    }
}
