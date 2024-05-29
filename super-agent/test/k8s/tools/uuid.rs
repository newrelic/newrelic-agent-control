use crate::tools::runtime::tokio_runtime;
use crate::tools::super_agent::TEST_CLUSTER_NAME;
use newrelic_super_agent::k8s::client::SyncK8sClient;
use newrelic_super_agent::k8s::store::K8sStore;
use newrelic_super_agent::opamp::instance_id;
use newrelic_super_agent::opamp::instance_id::getter::{InstanceIDGetter, ULIDInstanceIDGetter};
use newrelic_super_agent::opamp::instance_id::InstanceID;
use newrelic_super_agent::super_agent::config::AgentID;
use std::sync::Arc;

pub fn get_instance_id(namespace: &str, agent_id: &AgentID) -> InstanceID {
    let k8s_client = Arc::new(
        SyncK8sClient::try_new(tokio_runtime(), namespace.to_string(), Vec::new()).unwrap(),
    );
    let instance_id_getter = ULIDInstanceIDGetter::new_k8s_instance_id_getter(
        Arc::new(K8sStore::new(k8s_client)),
        instance_id::get_identifiers(TEST_CLUSTER_NAME.to_string(), "".to_string()),
    );
    instance_id_getter.get(agent_id).unwrap()
}
