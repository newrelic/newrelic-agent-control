use crate::common::retry::retry;
use fs::directory_manager::DirectoryManagerFs;
use fs::LocalFile;
use newrelic_super_agent::opamp::instance_id::storer::InstanceIDStorer;
use newrelic_super_agent::opamp::instance_id::{InstanceID, Storer};
use newrelic_super_agent::super_agent::config::AgentID;
use std::error::Error;
use std::time::Duration;

pub fn get_instance_id(agent_id: &AgentID) -> InstanceID {
    let instance_id_storer: Storer<LocalFile, DirectoryManagerFs> = Storer::default();
    let mut super_agent_instance_id: InstanceID = InstanceID::create();
    retry(15, Duration::from_secs(1), || {
        || -> Result<(), Box<dyn Error>> {
            super_agent_instance_id = instance_id_storer
                .get(agent_id)?
                .ok_or("SA instance id missing")?
                .instance_id;
            Ok(())
        }()
    });
    super_agent_instance_id
}
