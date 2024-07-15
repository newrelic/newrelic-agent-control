use crate::common::retry::retry;
use fs::directory_manager::DirectoryManagerFs;
use fs::LocalFile;
use newrelic_super_agent::opamp::instance_id::storer::InstanceIDStorer;
use newrelic_super_agent::opamp::instance_id::{InstanceID, Storer};
use newrelic_super_agent::super_agent::config::AgentID;
use newrelic_super_agent::super_agent::defaults::SUB_AGENT_DIR;
use newrelic_super_agent::super_agent::run::BasePaths;
use std::error::Error;
use std::time::Duration;

pub fn get_instance_id(agent_id: &AgentID, base_paths: BasePaths) -> InstanceID {
    let instance_id_storer = Storer::new(
        LocalFile,
        DirectoryManagerFs::default(),
        base_paths.remote_dir.clone(),
        base_paths.remote_dir.join(SUB_AGENT_DIR()),
    );

    let mut super_agent_instance_id: InstanceID = InstanceID::create();
    retry(30, Duration::from_secs(1), || {
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
