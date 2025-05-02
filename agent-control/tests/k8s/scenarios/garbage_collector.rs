use std::time::Duration;

use kube::Api;
use newrelic_agent_control::{agent_control::agent_id::AgentID, k8s::labels::Labels};
use tempfile::tempdir;

use crate::{
    common::{opamp::FakeServer, retry::retry, runtime::block_on},
    k8s::tools::{
        agent_control::{
            CUSTOM_AGENT_TYPE_PATH, start_agent_control_with_testdata_config,
            wait_until_agent_control_with_opamp_is_started,
        },
        k8s_env::K8sEnv,
        test_crd::{Foo, create_foo_cr},
    },
};

#[test]
#[ignore = "needs k8s cluster"]
/// Tests that resources that already exist in the cluster of agents that are no longer active are removed.
fn k8s_garbage_collector_triggers_on_ac_startup() {
    let test_name = "k8s_garbage_collector_triggers_on_ac_startup";
    let mut k8s = block_on(K8sEnv::new());
    let test_ns = block_on(k8s.test_namespace());

    // Creates CRs labeled for two agents.
    let removed_agent_id = "removed";
    block_on(create_foo_cr(
        k8s.client.clone(),
        &test_ns,
        removed_agent_id,
        Some(Labels::new(&AgentID::try_from(removed_agent_id.to_string()).unwrap()).get()),
        None,
    ));

    // start Agent Control, so the objects above should be removed by the GC.
    let tmp_dir = tempdir().expect("failed to create local temp dir");
    let server = FakeServer::start_new();
    let _sa = start_agent_control_with_testdata_config(
        test_name,
        CUSTOM_AGENT_TYPE_PATH,
        k8s.client.clone(),
        &test_ns,
        Some(server.cert_file_path()),
        Some(&server.endpoint()),
        // This config is intended to be empty
        vec![],
        tmp_dir.path(),
    );
    wait_until_agent_control_with_opamp_is_started(k8s.client.clone(), test_ns.as_str());

    let api: Api<Foo> = Api::namespaced(k8s.client.clone(), &test_ns);
    retry(30, Duration::from_secs(1), || {
        if block_on(api.get(removed_agent_id)).is_ok() {
            return Err("agent should have been removed".into());
        }
        Ok(())
    });
}
