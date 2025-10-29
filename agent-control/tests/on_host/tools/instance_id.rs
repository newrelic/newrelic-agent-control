use crate::common::retry::retry;
use fs::LocalFile;
use fs::directory_manager::DirectoryManagerFs;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::on_host::file_store::FileStore;
use newrelic_agent_control::opamp::instance_id::InstanceID;
use newrelic_agent_control::opamp::instance_id::on_host::identifiers::Identifiers;
use newrelic_agent_control::opamp::instance_id::storer::{GenericStorer, InstanceIDStorer};
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

pub fn get_instance_id(agent_id: &AgentID, base_paths: BasePaths) -> InstanceID {
    let file_store = Arc::new(FileStore::new(
        LocalFile,
        DirectoryManagerFs,
        base_paths.local_dir.clone(),
        base_paths.remote_dir.clone(),
    ));
    let instance_id_storer: GenericStorer<FileStore<LocalFile, DirectoryManagerFs>, Identifiers> =
        GenericStorer::from(file_store);

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
