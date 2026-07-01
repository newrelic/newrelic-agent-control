use crate::common::{retry::retry, runtime::block_on};
use crate::k8s::tools::agent_control::CUSTOM_AGENT_TYPE_SPLIT_NS_PATH;
use crate::k8s::tools::{
    agent_control::{create_config_map, start_agent_control},
    config::K8sAgentControlConfigBuilder,
    k8s_api::check_deployments_exist,
    k8s_env::K8sEnv,
};
use std::time::Duration;
use tempfile::tempdir;

/// This scenario checks that the agent-control is executed and creates the k8s resources,
/// including the secret that is honored to modify the Deployment Name, when OpAMP is not enabled.
#[test]
#[ignore = "needs a k8s cluster"]
fn k8s_sub_agent_started_with_no_opamp() {
    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    let agents = r#"
  hello-world:
    agent_type: "newrelic/com.newrelic.custom_agent:0.0.1"
"#;

    K8sAgentControlConfigBuilder::new(&namespace)
        .with_agents(agents)
        .write(k8s.client.clone(), tmp_dir.path());

    block_on(create_config_map(
        k8s.client.clone(),
        &namespace,
        "local-data-hello-world",
        "chart_values: \n  nameOverride: from-local\n".to_string(),
    ));

    let _child = start_agent_control(
        CUSTOM_AGENT_TYPE_SPLIT_NS_PATH,
        k8s.client.clone(),
        &namespace,
        tmp_dir.path(),
    );

    // Check deployment for first Agent is created with retry, the name has the key
    // 'from-local' concatenated to the name because the secret created adds that
    // NameOverride to the values.
    retry(30, Duration::from_secs(1), || {
        check_deployments_exist(
            k8s.client.clone(),
            &["hello-world-from-local"],
            namespace.as_str(),
        )
    });
}
