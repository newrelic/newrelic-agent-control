use fs::LocalFile;
use log::error;
use std::collections::HashMap;
use std::path::Path;

use thiserror::Error;

use newrelic_super_agent::config::agent_type::agent_types::{AgentTypeEndSpec, VariableType};
use newrelic_super_agent::config::agent_type_registry::{
    AgentRegistry, AgentRepositoryError, LocalRegistry,
};

use fs::file_reader::{FileReader, FileReaderError};

use crate::migration::agent_value_spec::AgentValueSpec::AgentValueSpecEnd;
use crate::migration::agent_value_spec::{
    from_fqn_and_value, merge_agent_values, AgentValueError, AgentValueSpec,
};
use crate::migration::config::{AgentTypeFieldFQN, DirPath, FilePath, MigrationAgentConfig};
use crate::migration::config::{FILE_SEPARATOR, FILE_SEPARATOR_REPLACE};
use crate::migration::converter::ConversionError::RequiredFileMappingNotFoundError;

#[derive(Error, Debug)]
pub enum ConversionError {
    #[error("`{0}`")]
    RepositoryError(#[from] AgentRepositoryError),
    #[error("`{0}`")]
    ConvertFileError(#[from] FileReaderError),
    #[error("`{0}`")]
    AgentValueError(#[from] AgentValueError),
    #[error("cannot find required file map")]
    RequiredFileMappingNotFoundError,
}

pub struct ConfigConverter<R: AgentRegistry, F: FileReader> {
    agent_registry: R,
    file_reader: F,
}

impl Default for ConfigConverter<LocalRegistry, LocalFile> {
    fn default() -> Self {
        ConfigConverter {
            agent_registry: LocalRegistry::default(),
            file_reader: LocalFile,
        }
    }
}

#[cfg_attr(test, mockall::automock)]
impl<R: AgentRegistry, F: FileReader> ConfigConverter<R, F> {
    pub fn convert(
        &self,
        migration_agent_config: &MigrationAgentConfig,
    ) -> Result<HashMap<String, AgentValueSpec>, ConversionError> {
        let agent_type = self
            .agent_registry
            .get(&migration_agent_config.get_agent_type_fqn())?;

        let mut agent_values_specs: Vec<HashMap<String, AgentValueSpec>> = Vec::new();
        for (normalized_fqn, spec) in agent_type.variables.iter() {
            let agent_type_fqn: AgentTypeFieldFQN = normalized_fqn.into();
            match spec.variable_type() {
                VariableType::File => {
                    // look for file mapping, if not found and required throw an error
                    let file_map = migration_agent_config.get_file(agent_type_fqn.clone());
                    if spec.required && file_map.is_none() {
                        return Err(RequiredFileMappingNotFoundError);
                    }
                    agent_values_specs
                        .push(self.file_to_agent_value_spec(agent_type_fqn, file_map.unwrap())?)
                }
                VariableType::MapStringFile => {
                    // look for file mapping, if not found and required throw an error
                    let file_map = migration_agent_config.get_dir(agent_type_fqn.clone());
                    if spec.required && file_map.is_none() {
                        return Err(RequiredFileMappingNotFoundError);
                    }
                    agent_values_specs
                        .push(self.dir_to_agent_value_spec(agent_type_fqn, file_map.unwrap())?)
                }
                _ => {
                    error!("cannot handle variable type {:?}", spec.variable_type())
                }
            }
        }

        Ok(merge_agent_values(agent_values_specs)?)
    }

    fn file_to_agent_value_spec(
        &self,
        agent_type_field_fqn: AgentTypeFieldFQN,
        file_path: FilePath,
    ) -> Result<HashMap<String, AgentValueSpec>, ConversionError> {
        let contents = self.file_reader.read(Path::new(file_path.as_str()))?;
        Ok(from_fqn_and_value(
            agent_type_field_fqn.clone(),
            AgentValueSpecEnd(contents),
        ))
    }

    fn dir_to_agent_value_spec(
        &self,
        agent_type_field_fqn: AgentTypeFieldFQN,
        dir_path: DirPath,
    ) -> Result<HashMap<String, AgentValueSpec>, ConversionError> {
        let files_paths = self.file_reader.read_dir(Path::new(dir_path.as_str()))?;
        let mut res: Vec<HashMap<String, AgentValueSpec>> = Vec::new();
        // refactor file_path to path
        for file in files_paths {
            let path = Path::new(file.as_str());
            let filename = path.file_name().unwrap().to_str().unwrap().to_string();
            // replace the file separator to not be treated as a leaf
            let escaped_filename = filename.replace(FILE_SEPARATOR, FILE_SEPARATOR_REPLACE);
            let full_agent_type_field_fqn: AgentTypeFieldFQN =
                format!("{}.{}", agent_type_field_fqn, escaped_filename).into();
            res.push(self.file_to_agent_value_spec(full_agent_type_field_fqn, file)?);
        }
        Ok(merge_agent_values(res)?)
    }
}
