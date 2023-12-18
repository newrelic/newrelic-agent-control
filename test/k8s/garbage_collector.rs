use crate::common::{create_test_cr, foo_type_meta, Foo, K8sEnv, MockSuperAgentConfigLoader};
use k8s_openapi::{api::core::v1::ConfigMap, Resource};
use kube::{api::Api, core::TypeMeta};
use mockall::Sequence;
use newrelic_super_agent::{
    config::super_agent_configs::SuperAgentConfig,
    k8s::{executor::K8sExecutor, garbage_collector::NotStartedK8sGarbageCollector},
    super_agent::defaults::SUPER_AGENT_ID,
};
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_garbage_collector_cleans_removed_agent() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    let agent_id = "sub-agent";
    create_test_cr(test.client.to_owned(), test_ns.as_str(), agent_id).await;

    let mut config_loader = MockSuperAgentConfigLoader::new();
    let config = format!(
        r#"
agents:
  {agent_id}:
    agent_type: test
"#
    );
    let mut seq = Sequence::new();

    // First call have the agent id in the config
    config_loader
        .expect_load()
        .times(1)
        .returning(move || Ok(serde_yaml::from_str::<SuperAgentConfig>(config.as_str()).unwrap()))
        .in_sequence(&mut seq);

    // Second call will not have agents
    config_loader
        .expect_load()
        .times(1)
        .returning(move || Ok(serde_yaml::from_str::<SuperAgentConfig>("agents: {}").unwrap()))
        .in_sequence(&mut seq);

    let gc = NotStartedK8sGarbageCollector::new(
        Arc::new(config_loader),
        Arc::new(
            K8sExecutor::try_new_with_reflectors(test_ns.to_string(), vec![foo_type_meta()])
                .await
                .unwrap(),
        ),
    );

    // Expects the GC to keep the agent cr which is in the config, event if looking for multiple kinds or that
    // are missing in the cluster.
    gc.collect().await.unwrap();
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    let _result = api.get(agent_id).await.expect("CR should exist");

    // Expect that the current_agent is removed on the second call.
    gc.collect().await.unwrap();
    api.get(agent_id).await.expect_err("CR should be removed");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_garbage_collector_with_missing_and_extra_kinds() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    // Creates CRs labeled for two agents.
    let removed_agent_id = "removed";
    create_test_cr(test.client.to_owned(), test_ns.as_str(), removed_agent_id).await;

    // Executes the GC passing only current agent in the config.
    let mut config_loader = MockSuperAgentConfigLoader::new();

    config_loader
        .expect_load()
        .times(1)
        .returning(move || Ok(serde_yaml::from_str::<SuperAgentConfig>("agents: {}").unwrap()));

    // This kind is not present in the cluster.
    let missing_kind = TypeMeta {
        api_version: "missing.com/v1".to_string(),
        kind: "Missing".to_string(),
    };

    let existing_kind = TypeMeta {
        api_version: ConfigMap::API_VERSION.to_string(),
        kind: ConfigMap::KIND.to_string(),
    };

    let gc = NotStartedK8sGarbageCollector::new(
        Arc::new(config_loader),
        Arc::new(
            K8sExecutor::try_new_with_reflectors(
                test_ns.to_string(),
                vec![foo_type_meta(), existing_kind, missing_kind],
            )
            .await
            .unwrap(),
        ),
    );

    // Expects the GC to clean the "removed" agent CR.
    gc.collect().await.unwrap();
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    api.get(removed_agent_id)
        .await
        .expect_err("fail garbage collecting removed agent");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs k8s cluster"]
async fn k8s_garbage_collector_does_not_remove_super_agent() {
    let mut test = K8sEnv::new().await;
    let test_ns = test.test_namespace().await;

    create_test_cr(test.client.to_owned(), test_ns.as_str(), SUPER_AGENT_ID).await;

    let mut config_loader = MockSuperAgentConfigLoader::new();

    config_loader
        .expect_load()
        .times(1)
        .returning(move || Ok(serde_yaml::from_str::<SuperAgentConfig>("agents: {}").unwrap()));

    let gc = NotStartedK8sGarbageCollector::new(
        Arc::new(config_loader),
        Arc::new(
            K8sExecutor::try_new_with_reflectors(test_ns.to_string(), vec![foo_type_meta()])
                .await
                .unwrap(),
        ),
    );

    // Expects the GC do not clean any resource related to the SA.
    gc.collect().await.unwrap();
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    api.get(SUPER_AGENT_ID).await.expect("CR should exist");
}
