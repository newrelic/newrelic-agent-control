use fs::directory_manager::DirectoryManagerFs;
use fs::LocalFile;
use newrelic_super_agent::opamp::instance_id::getter::{
    InstanceIDGetter, InstanceIDWithIdentifiersGetter,
};
use newrelic_super_agent::opamp::instance_id::{IdentifiersProvider, InstanceID, Storer};
use newrelic_super_agent::super_agent::config::AgentID;
use newrelic_super_agent::super_agent::defaults::{SUB_AGENT_DIR, SUPER_AGENT_DATA_DIR};
use std::path::PathBuf;

pub fn get_instance_id(agent_id: &AgentID) -> InstanceID {
    let identifiers_provider = IdentifiersProvider::default()
        .with_host_id("integration-test".to_string())
        .with_fleet_id("integration".to_string());
    let identifiers = identifiers_provider.provide().unwrap_or_default();

    let instance_id_storer = Storer::new(
        LocalFile,
        DirectoryManagerFs::default(),
        PathBuf::from(SUPER_AGENT_DATA_DIR()),
        PathBuf::from(SUPER_AGENT_DATA_DIR()).join(SUB_AGENT_DIR()),
    );

    let instance_id_getter = InstanceIDWithIdentifiersGetter::new(instance_id_storer, identifiers);

    instance_id_getter.get(agent_id).unwrap()
}
