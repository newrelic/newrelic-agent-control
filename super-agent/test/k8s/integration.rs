use crate::common::{
    block_on, check_deployments_exist, create_mock_config_maps, start_super_agent, K8sEnv,
};
use newrelic_super_agent::k8s::store::STORE_KEY_LOCAL_DATA_CONFIG;
use std::path::Path;
use std::time::Duration;

#[test]
#[ignore = "needs a k8s cluster"]
fn k8s_sub_agent_started() {
    let file_path = Path::new("test/k8s/data/static.yml");
    // Setup k8s env
    let k8s = block_on(K8sEnv::new());
    let namespace = "default";

    block_on(create_mock_config_maps(
        k8s.client.clone(),
        namespace,
        "local-data-my-agent-id",
        STORE_KEY_LOCAL_DATA_CONFIG,
    ));
    block_on(create_mock_config_maps(
        k8s.client.clone(),
        namespace,
        "local-data-my-agent-id-2",
        STORE_KEY_LOCAL_DATA_CONFIG,
    ));

    let mut child = start_super_agent(file_path);

    let deployment_name = "my-agent-id-opentelemetry-collector";
    let deployment_name_2 = "my-agent-id-2-opentelemetry-collector";

    let max_retries = 30;
    let duration = Duration::from_millis(5000);

    // Check deployment for first Agent is created with retry.
    block_on(check_deployments_exist(
        k8s.client.clone(),
        &[deployment_name],
        namespace,
        max_retries,
        duration,
    ));

    // Check deployment for second Agent is created with retry.
    block_on(check_deployments_exist(
        k8s.client.clone(),
        &[deployment_name_2],
        namespace,
        max_retries,
        duration,
    ));

    child.kill().expect("Failed to kill child process");

    // TODO Clean resources after finish when working with this test in the future.
}
