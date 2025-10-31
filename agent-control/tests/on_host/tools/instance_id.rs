use crate::common::retry::retry;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::STORE_KEY_INSTANCE_ID;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::on_host::file_store::FileStore;
use newrelic_agent_control::opamp::data_store::OpAMPDataStore;
use newrelic_agent_control::opamp::instance_id::InstanceID;
use newrelic_agent_control::opamp::instance_id::getter::DataStored;
use newrelic_agent_control::opamp::instance_id::on_host::identifiers::Identifiers;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

pub fn get_instance_id(agent_id: &AgentID, base_paths: BasePaths) -> InstanceID {
    let file_store = Arc::new(FileStore::new_local_fs(
        base_paths.local_dir.clone(),
        base_paths.remote_dir.clone(),
    ));

    let mut agent_control_instance_id: InstanceID = InstanceID::create();
    retry(30, Duration::from_secs(1), || {
        || -> Result<(), Box<dyn Error>> {
            agent_control_instance_id = file_store
                .get_opamp_data::<DataStored<Identifiers>>(agent_id, STORE_KEY_INSTANCE_ID)?
                .ok_or("SA instance id missing")?
                .instance_id;
            Ok(())
        }()
    });

    agent_control_instance_id
}
