use crate::agent_control::run::Environment;
use crate::agent_type::agent_type_registry::{AgentRegistry, AgentRepositoryError};
use crate::agent_type::embedded_registry::EmbeddedRegistry;
use crate::agent_type::variable::kind::Kind;
use crate::config_migrate::migration::agent_value_spec::AgentValueSpec::AgentValueSpecEnd;
use crate::config_migrate::migration::agent_value_spec::{
    AgentValueError, AgentValueSpec, from_fqn_and_value, merge_agent_values,
};
use crate::config_migrate::migration::config::{
    AgentTypeFieldFQN, DirInfo, FilePath, MigrationAgentConfig,
};
use crate::config_migrate::migration::config::{FILE_SEPARATOR, FILE_SEPARATOR_REPLACE};
use crate::config_migrate::migration::converter::ConversionError::RequiredFileMappingNotFoundError;
use crate::sub_agent::effective_agents_assembler::{AgentTypeDefinitionError, build_agent_type};
use fs::LocalFile;
use fs::file_reader::{FileReader, FileReaderError};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, error};

#[derive(Error, Debug)]
pub enum ConversionError {
    #[error("`{0}`")]
    RepositoryError(#[from] AgentRepositoryError),
    #[error("`{0}`")]
    ConvertFileError(#[from] FileReaderError),
    #[error("`{0}`")]
    AgentValueError(#[from] AgentValueError),
    #[error("`{0}`")]
    AgentTypeDefinitionError(#[from] AgentTypeDefinitionError),
    #[error("cannot find required file map")]
    RequiredFileMappingNotFoundError,
}

pub struct ConfigConverter<R: AgentRegistry, F: FileReader> {
    agent_registry: R,
    file_reader: F,
}

impl Default for ConfigConverter<EmbeddedRegistry, LocalFile> {
    fn default() -> Self {
        ConfigConverter {
            agent_registry: EmbeddedRegistry::default(),
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
        let agent_type_definition = self
            .agent_registry
            .get(&migration_agent_config.get_agent_type_fqn().to_string())?;

        let agent_type = build_agent_type(agent_type_definition, &Environment::OnHost)?;
        let mut agent_values_specs: Vec<HashMap<String, AgentValueSpec>> = Vec::new();
        for (normalized_fqn, spec) in agent_type.variables.flatten().iter() {
            let agent_type_fqn: AgentTypeFieldFQN = normalized_fqn.into();
            match spec.kind() {
                Kind::File(_) => {
                    // look for file mapping, if not found and required throw an error
                    let file_map = migration_agent_config.get_file(agent_type_fqn.clone());
                    if spec.is_required() && file_map.is_none() {
                        return Err(RequiredFileMappingNotFoundError);
                    }
                    agent_values_specs
                        .push(self.file_to_agent_value_spec(agent_type_fqn, file_map.unwrap())?)
                }
                Kind::MapStringFile(_) => {
                    // look for file mapping, if not found and required throw an error
                    let dir_info = migration_agent_config.get_dir(agent_type_fqn.clone());
                    if spec.is_required() && dir_info.is_none() {
                        return Err(RequiredFileMappingNotFoundError);
                    }
                    agent_values_specs
                        .push(self.dir_to_agent_value_spec(agent_type_fqn, dir_info.unwrap())?)
                }
                _ => {
                    debug!("skipping variable {}", agent_type_fqn.as_string())
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
        let contents = self.file_reader.read(file_path.as_path())?;
        Ok(from_fqn_and_value(
            agent_type_field_fqn.clone(),
            AgentValueSpecEnd(contents),
        ))
    }

    fn dir_to_agent_value_spec(
        &self,
        agent_type_field_fqn: AgentTypeFieldFQN,
        dir_info: DirInfo,
    ) -> Result<HashMap<String, AgentValueSpec>, ConversionError> {
        let files_paths = self.file_reader.dir_entries(dir_info.path.as_path())?;
        let mut res: Vec<HashMap<String, AgentValueSpec>> = Vec::new();
        // refactor file_path to path
        for path in files_paths {
            let filename = path.file_name().unwrap().to_str().unwrap().to_string();
            //filter by filename
            if !dir_info.valid_filename(filename.as_str()) {
                continue;
            }

            // replace the file separator to not be treated as a leaf
            let escaped_filename = filename.replace(FILE_SEPARATOR, FILE_SEPARATOR_REPLACE);
            let full_agent_type_field_fqn: AgentTypeFieldFQN =
                format!("{}.{}", agent_type_field_fqn, escaped_filename).into();
            res.push(self.file_to_agent_value_spec(full_agent_type_field_fqn, path)?);
        }
        Ok(merge_agent_values(res)?)
    }
}
