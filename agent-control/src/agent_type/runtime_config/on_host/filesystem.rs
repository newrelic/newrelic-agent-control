use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Deserializer};

use crate::agent_type::{
    definition::Variables, error::AgentTypeError,
    runtime_config::templateable_value::TemplateableValue, templates::Templateable,
};

/// Represents the file system configuration for the deployment of an agent.
///
/// It is a key-value structure in which every key is an identifier and the value is a file entry.
/// See [FileEntry] for details.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileSystem(HashMap<String, FileEntry>);

/// A file entry consists on a path and its content. The path must always be relative,
/// as these represent files that will be created for a sub-agent's scope (i.e. in AC's
/// auto-generated directory for that sub-agent).
#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
struct FileEntry {
    path: PathBuf,
    content: TemplateableValue<String>,
}

impl<'de> Deserialize<'de> for FileSystem {
    fn deserialize<D>(deserializer: D) -> Result<FileSystem, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        let map = HashMap::<_, FileEntry>::deserialize(deserializer)?;
        // Perform validations on the provided Paths
        if map.values().map(|v| v.path.as_ref()).all(Path::is_relative) {
            // TODO more validations (not exist, etc?)
            Ok(FileSystem(map))
        } else {
            Err(Error::custom("All paths used as keys must be relative"))
        }
    }
}

impl Templateable for FileSystem {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        self.0
            .into_iter()
            .map(|(k, v)| Ok((k, v.template_with(variables)?)))
            .collect::<Result<HashMap<_, _>, _>>()
            .map(FileSystem)
    }
}

impl Templateable for FileEntry {
    fn template_with(self, variables: &Variables) -> Result<Self, AgentTypeError> {
        Ok(Self {
            path: self.path,
            content: self.content.template_with(variables)?,
        })
    }
}
