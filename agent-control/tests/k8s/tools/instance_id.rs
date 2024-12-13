use crate::common::retry::retry;
use crate::common::runtime::tokio_runtime;
use newrelic_agent_control::agent_control::config::AgentID;
use newrelic_agent_control::k8s::client::SyncK8sClient;
use newrelic_agent_control::k8s::store::{K8sStore, STORE_KEY_INSTANCE_ID};
use newrelic_agent_control::opamp::instance_id::getter::DataStored;
use newrelic_agent_control::opamp::instance_id::InstanceID;
use std::sync::Arc;
use std::time::Duration;

pub fn get_instance_id(namespace: &str, agent_id: &AgentID) -> InstanceID {
    let k8s_client =
        Arc::new(SyncK8sClient::try_new(tokio_runtime(), namespace.to_string()).unwrap());
    let store = K8sStore::new(k8s_client);

    let mut id = InstanceID::create();

    retry(60, Duration::from_secs(1), || {
        let data: Option<DataStored> = store.get_opamp_data(agent_id, STORE_KEY_INSTANCE_ID)?;
        id = data
            .ok_or(format!("agent_id={} Instance ID not found", agent_id))?
            .instance_id;
        Ok(())
    });
    id
}
