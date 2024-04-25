use crate::common::k8s_env;

use super::common::{create_test_cr, MockSuperAgentDynamicConfigLoaderMock};
use k8s_openapi::{api::core::v1::ConfigMap, Resource};
use k8s_test_env::foo_crd::{foo_type_meta, Foo};
use k8s_test_env::runtime::{block_on, tokio_runtime};
use kube::{api::Api, core::TypeMeta};
use mockall::Sequence;
use newrelic_super_agent::super_agent::config::SuperAgentDynamicConfig;
use newrelic_super_agent::{
    agent_type::runtime_config::K8sObject,
    k8s::{
        client::SyncK8sClient, garbage_collector::NotStartedK8sGarbageCollector, store::K8sStore,
    },
    opamp::instance_id::{
        getter::{InstanceIDGetter, ULIDInstanceIDGetter},
        Identifiers,
    },
    sub_agent::k8s::CRSupervisor,
    super_agent::{config::AgentID, defaults::SUPER_AGENT_ID},
};
use std::{collections::HashMap, sync::Arc};

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_garbage_collector_cleans_removed_agent() {
    let mut test = block_on(k8s_env());
    let test_ns = block_on(test.test_namespace());

    let agent_id = &AgentID::new("sub-agent").unwrap();

    let k8s_client = Arc::new(
        SyncK8sClient::try_new_with_reflectors(
            tokio_runtime(),
            test_ns.to_string(),
            vec![foo_type_meta()],
        )
        .unwrap(),
    );

    let resource_name = "test-different-from-agent-id";
    let s = CRSupervisor::new(
        agent_id.clone(),
        k8s_client.clone(),
        HashMap::from([(
            "fooCR".to_string(),
            serde_yaml::from_str::<K8sObject>(
                format!(
                    r#"
apiVersion: {}
kind: {}
spec:
    data: test
metadata:
  name: {}
        "#,
                    foo_type_meta().api_version,
                    foo_type_meta().kind,
                    resource_name,
                )
                .as_str(),
            )
            .unwrap(),
        )]),
    );

    // Creates the Foo CR correctly tagged.
    s.apply().unwrap();

    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

    let instance_id_getter =
        ULIDInstanceIDGetter::try_with_identifiers(k8s_store.clone(), Identifiers::default())
            .unwrap();

    // Creates ULID CM correctly tagged.
    let agent_ulid = instance_id_getter.get(agent_id).unwrap();

    let mut config_loader = MockSuperAgentDynamicConfigLoaderMock::new();
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
        .returning(move || {
            Ok(serde_yaml::from_str::<SuperAgentDynamicConfig>(config.as_str()).unwrap())
        })
        .in_sequence(&mut seq);

    // Second call will not have agents
    config_loader
        .expect_load()
        .times(1)
        .returning(move || {
            Ok(serde_yaml::from_str::<SuperAgentDynamicConfig>("agents: {}").unwrap())
        })
        .in_sequence(&mut seq);

    let mut gc = NotStartedK8sGarbageCollector::new(Arc::new(config_loader), k8s_client);

    // Expects the GC to keep the agent cr which is in the config, event if looking for multiple kinds or that
    // are missing in the cluster.
    gc.collect().unwrap();
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    block_on(api.get(resource_name)).expect("CR should exist");
    assert_eq!(
        agent_ulid,
        instance_id_getter.get(agent_id).unwrap(),
        "Expects the ULID keeps the same since is get from the CM"
    );

    // Expect that the current_agent is removed on the second call.
    gc.collect().unwrap();
    block_on(api.get(resource_name)).expect_err("CR should be removed");
    assert_ne!(
        agent_ulid,
        instance_id_getter.get(agent_id).unwrap(),
        "Expects the new ULID is generated after the CM removal"
    );
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_garbage_collector_with_missing_and_extra_kinds() {
    let mut test = block_on(k8s_env());
    let test_ns = block_on(test.test_namespace());

    // Creates CRs labeled for two agents.
    let removed_agent_id = "removed";
    block_on(create_test_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        removed_agent_id,
    ));

    // Executes the GC passing only current agent in the config.
    let mut config_loader = MockSuperAgentDynamicConfigLoaderMock::new();

    config_loader.expect_load().times(1).returning(move || {
        Ok(serde_yaml::from_str::<SuperAgentDynamicConfig>("agents: {}").unwrap())
    });

    // This kind is not present in the cluster.
    let missing_kind = TypeMeta {
        api_version: "missing.com/v1".to_string(),
        kind: "Missing".to_string(),
    };

    let existing_kind = TypeMeta {
        api_version: ConfigMap::API_VERSION.to_string(),
        kind: ConfigMap::KIND.to_string(),
    };

    let mut gc = NotStartedK8sGarbageCollector::new(
        Arc::new(config_loader),
        Arc::new(
            SyncK8sClient::try_new_with_reflectors(
                tokio_runtime(),
                test_ns.to_string(),
                vec![foo_type_meta(), existing_kind, missing_kind],
            )
            .unwrap(),
        ),
    );

    // Expects the GC to clean the "removed" agent CR.
    gc.collect().unwrap();
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    block_on(api.get(removed_agent_id)).expect_err("fail garbage collecting removed agent");
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_garbage_collector_does_not_remove_super_agent() {
    let mut test = block_on(k8s_env());
    let test_ns = block_on(test.test_namespace());

    let sa_id = &AgentID::new_super_agent_id();
    block_on(create_test_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        sa_id,
    ));

    let k8s_client = Arc::new(
        SyncK8sClient::try_new_with_reflectors(
            tokio_runtime(),
            test_ns.to_string(),
            vec![foo_type_meta()],
        )
        .unwrap(),
    );
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

    let instance_id_getter =
        ULIDInstanceIDGetter::try_with_identifiers(k8s_store.clone(), Identifiers::default())
            .unwrap();

    let sa_ulid = instance_id_getter.get(sa_id).unwrap();

    let mut config_loader = MockSuperAgentDynamicConfigLoaderMock::new();

    config_loader.expect_load().times(1).returning(move || {
        Ok(serde_yaml::from_str::<SuperAgentDynamicConfig>("agents: {}").unwrap())
    });

    let mut gc = NotStartedK8sGarbageCollector::new(Arc::new(config_loader), k8s_client);

    // Expects the GC do not clean any resource related to the SA.
    gc.collect().unwrap();
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    block_on(api.get(SUPER_AGENT_ID)).expect("CR should exist");
    assert_eq!(
        sa_ulid,
        instance_id_getter.get(sa_id).unwrap(),
        "Expects the ULID keeps the same since is get from the CM"
    );
}
