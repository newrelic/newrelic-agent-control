use crate::common::retry::retry;
use fs::directory_manager::DirectoryManagerFs;
use fs::LocalFile;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::SUB_AGENT_DIR;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::opamp::instance_id::on_host::storer::Storer;
use newrelic_agent_control::opamp::instance_id::storer::InstanceIDStorer;
use newrelic_agent_control::opamp::instance_id::InstanceID;
use std::error::Error;
use std::time::Duration;

pub fn get_instance_id(agent_id: &AgentID, base_paths: BasePaths) -> InstanceID {
    let instance_id_storer = Storer::new(
        LocalFile,
        DirectoryManagerFs,
        base_paths.remote_dir.clone(),
        base_paths.remote_dir.join(SUB_AGENT_DIR),
    );

    let mut agent_control_instance_id: InstanceID = InstanceID::create();
    retry(30, Duration::from_secs(1), || {
        || -> Result<(), Box<dyn Error>> {
            agent_control_instance_id = instance_id_storer
                .get(agent_id)?
                .ok_or("SA instance id missing")?
                .instance_id;
            Ok(())
        }()
    });

    agent_control_instance_id
}
