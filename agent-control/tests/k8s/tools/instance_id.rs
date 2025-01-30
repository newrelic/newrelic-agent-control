use crate::common::retry::retry;
use crate::common::runtime::block_on;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::{Api, Client};
use newrelic_agent_control::agent_control::config::AgentID;
use newrelic_agent_control::k8s::store::{
    K8sStore, CM_NAME_OPAMP_DATA_PREFIX, STORE_KEY_INSTANCE_ID,
};
use newrelic_agent_control::opamp::instance_id::getter::DataStored;
use newrelic_agent_control::opamp::instance_id::InstanceID;
use std::time::Duration;

pub fn get_instance_id(k8s_client: Client, namespace: &str, agent_id: &AgentID) -> InstanceID {
    let cm_client: Api<ConfigMap> = Api::<ConfigMap>::namespaced(k8s_client, namespace);

    let cm_name = K8sStore::build_cm_name(agent_id, CM_NAME_OPAMP_DATA_PREFIX);

    let mut id = InstanceID::create();

    let err = format!("agent_id={agent_id} Getting Instance ID");

    retry(60, Duration::from_secs(1), || {
        let cm = block_on(cm_client.get(&cm_name))?;

        let raw_identifiers = cm
            .data
            .ok_or(err.clone())?
            .get(STORE_KEY_INSTANCE_ID)
            .cloned()
            .ok_or(err.clone())?;

        let data_stored: DataStored = serde_yaml::from_str(raw_identifiers.as_str())?;

        id = data_stored.instance_id;

        Ok(())
    });
    id
}
