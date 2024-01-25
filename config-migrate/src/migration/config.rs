use newrelic_super_agent::super_agent::config::AgentTypeFQN;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};

pub const FILE_SEPARATOR: &str = ".";
// Used to replace temporarily the . separator on files to not treat them as leafs on the hashmap
pub const FILE_SEPARATOR_REPLACE: &str = "#";

pub type FilePath = String;
pub type DirPath = String;

#[derive(Debug, Clone, Deserialize)]
pub struct AgentTypeFieldFQN(String);

impl AgentTypeFieldFQN {
    pub fn as_string(&self) -> String {
        self.0.clone()
    }
}

impl Display for AgentTypeFieldFQN {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl From<String> for AgentTypeFieldFQN {
    fn from(value: String) -> Self {
        AgentTypeFieldFQN(value.to_string())
    }
}

impl From<&String> for AgentTypeFieldFQN {
    fn from(value: &String) -> Self {
        AgentTypeFieldFQN(value.to_string())
    }
}

impl From<&str> for AgentTypeFieldFQN {
    fn from(value: &str) -> Self {
        AgentTypeFieldFQN(value.to_string())
    }
}

impl PartialEq for AgentTypeFieldFQN {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl AgentTypeFieldFQN {
    pub fn as_vec(&self) -> Vec<&str> {
        self.0.split(FILE_SEPARATOR).collect::<Vec<&str>>()
    }
}

impl Eq for AgentTypeFieldFQN {}

impl Hash for AgentTypeFieldFQN {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }

    fn hash_slice<H: Hasher>(data: &[Self], state: &mut H)
    where
        Self: Sized,
    {
        for piece in data {
            piece.hash(state)
        }
    }
}

pub struct FileMap {
    pub file_path: FilePath,
    pub agent_type_fqn: AgentTypeFQN,
}

pub struct DirMap {
    pub file_path: FilePath,
    pub agent_type_fqn: AgentTypeFQN,
}

pub type FilesMap = HashMap<AgentTypeFieldFQN, FilePath>;
pub type DirsMap = HashMap<AgentTypeFieldFQN, DirPath>;

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub struct MigrationConfig {
    pub configs: Vec<MigrationAgentConfig>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub struct MigrationAgentConfig {
    pub agent_type_fqn: AgentTypeFQN,
    pub files_map: FilesMap,
    pub dirs_map: DirsMap,
}

impl MigrationAgentConfig {
    pub(crate) fn get_agent_type_fqn(&self) -> AgentTypeFQN {
        //AgentTypeFQN::from(self.agent_type_fqn.as_str())
        self.agent_type_fqn.clone()
    }
}

impl MigrationAgentConfig {
    pub fn get_file(&self, fqn_to_check: AgentTypeFieldFQN) -> Option<FilePath> {
        for (fqn, path) in self.files_map.iter() {
            if *fqn == fqn_to_check {
                return Some(path.clone());
            }
        }
        None
    }

    pub fn get_dir(&self, fqn_to_check: AgentTypeFieldFQN) -> Option<DirPath> {
        for (fqn, path) in self.dirs_map.iter() {
            if *fqn == fqn_to_check {
                return Some(path.clone());
            }
        }
        None
    }
}
