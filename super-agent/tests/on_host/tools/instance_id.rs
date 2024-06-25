use fs::directory_manager::{DirectoryManager, DirectoryManagerFs};
use fs::file_reader::FileReader;
use fs::writer_file::FileWriter;
use fs::LocalFile;
use newrelic_super_agent::opamp::instance_id::getter::{
    DataStored, InstanceIDGetter, InstanceIDWithIdentifiersGetter,
};
use newrelic_super_agent::opamp::instance_id::storer::InstanceIDStorer;
use newrelic_super_agent::opamp::instance_id::{IdentifiersProvider, InstanceID};
use newrelic_super_agent::super_agent::config::AgentID;
use newrelic_super_agent::super_agent::defaults::{
    REMOTE_AGENT_DATA_DIR, SUPER_AGENT_IDENTIFIERS_PATH,
};
use std::path::PathBuf;
use tracing::debug;

pub struct Storer<F = LocalFile, D = DirectoryManagerFs>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    file_rw: F,
    _dir_manager: D,
}

fn get_instance_id_path(agent_id: &AgentID) -> PathBuf {
    if agent_id.is_super_agent_id() {
        PathBuf::from(SUPER_AGENT_IDENTIFIERS_PATH())
    } else {
        PathBuf::from(format!(
            "{}/{}/identifiers.yaml",
            REMOTE_AGENT_DATA_DIR(),
            agent_id.get()
        ))
    }
}

impl<F, D> InstanceIDStorer for Storer<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    fn set(
        &self,
        _agent_id: &AgentID,
        _ds: &DataStored,
    ) -> Result<(), newrelic_super_agent::opamp::instance_id::StorerError> {
        Ok(())
    }

    fn get(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<DataStored>, newrelic_super_agent::opamp::instance_id::StorerError> {
        self.read_contents(agent_id)
    }
}

impl<F, D> Storer<F, D>
where
    D: DirectoryManager,
    F: FileWriter + FileReader,
{
    fn read_contents(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<DataStored>, newrelic_super_agent::opamp::instance_id::StorerError> {
        let dest_path = get_instance_id_path(agent_id);
        let file_str = match self.file_rw.read(dest_path.as_path()) {
            Ok(s) => s,
            Err(e) => {
                debug!("error reading file for agent {}: {}", agent_id, e);
                return Ok(None);
            }
        };
        match serde_yaml::from_str(&file_str) {
            Ok(ds) => Ok(Some(ds)),
            Err(e) => {
                debug!("error deserializing data for agent {}: {}", agent_id, e);
                Ok(None)
            }
        }
    }
}

pub fn get_instance_id(agent_id: &AgentID) -> InstanceID {
    let identifiers_provider = IdentifiersProvider::default()
        .with_host_id("integration-test".to_string())
        .with_fleet_id("integration".to_string());
    let identifiers = identifiers_provider.provide().unwrap_or_default();

    let instance_id_getter =
        InstanceIDWithIdentifiersGetter::default().with_identifiers(identifiers);

    instance_id_getter.get(agent_id).unwrap()
}
