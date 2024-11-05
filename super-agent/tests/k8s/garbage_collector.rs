use crate::common::runtime::{block_on, tokio_runtime};

use super::tools::{
    foo_crd::{create_foo_cr, foo_type_meta, Foo},
    k8s_env::K8sEnv,
};
use k8s_openapi::{api::core::v1::ConfigMap, Resource};
use kube::{api::Api, core::TypeMeta};
use mockall::{mock, Sequence};
use newrelic_super_agent::agent_type::runtime_config;
use newrelic_super_agent::k8s::annotations::Annotations;
use newrelic_super_agent::sub_agent::k8s::NotStartedSupervisorK8s;
use newrelic_super_agent::super_agent::config::AgentTypeFQN;
use newrelic_super_agent::{
    agent_type::runtime_config::K8sObject,
    k8s::{
        client::SyncK8sClient, garbage_collector::NotStartedK8sGarbageCollector, store::K8sStore,
    },
    opamp::instance_id::{
        getter::{InstanceIDGetter, InstanceIDWithIdentifiersGetter},
        Identifiers,
    },
    super_agent::{config::AgentID, defaults::SUPER_AGENT_ID},
};
use newrelic_super_agent::{
    k8s::labels::Labels,
    super_agent::{
        config::{SuperAgentConfig, SuperAgentConfigError, SuperAgentDynamicConfig},
        config_storer::loader_storer::{SuperAgentConfigLoader, SuperAgentDynamicConfigLoader},
    },
};
use std::{collections::HashMap, sync::Arc};

// Setup SuperAgentConfigLoader mock
mock! {
    pub SuperAgentConfigLoader {}

    impl SuperAgentConfigLoader for SuperAgentConfigLoader {
        fn load(&self) -> Result<SuperAgentConfig, SuperAgentConfigError>;
    }
}

// Setup SuperAgentDynamicConfigLoader mock
mock! {
    pub SuperAgentDynamicConfigLoaderMock{}

    impl SuperAgentDynamicConfigLoader for SuperAgentDynamicConfigLoaderMock {
        fn load(&self) -> Result<SuperAgentDynamicConfig, SuperAgentConfigError>;
    }
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_garbage_collector_cleans_removed_agent() {
    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let agent_id = &AgentID::new("sub-agent").unwrap();
    let agent_fqn = AgentTypeFQN::try_from("ns/test:1.2.3").unwrap();

    let k8s_client = Arc::new(
        SyncK8sClient::try_new(tokio_runtime(), test_ns.to_string(), vec![foo_type_meta()])
            .unwrap(),
    );

    let resource_name = "test-different-from-agent-id";

    let s = NotStartedSupervisorK8s::new(
        agent_id.clone(),
        agent_fqn.clone(),
        k8s_client.clone(),
        runtime_config::K8s {
            objects: HashMap::from([(
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
            health: None,
        },
    );

    // Creates the Foo CR correctly tagged.
    s.build_dynamic_objects()
        .unwrap()
        .iter()
        .try_for_each(|obj| k8s_client.apply_dynamic_object_if_changed(obj))
        .unwrap();

    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

    let instance_id_getter = InstanceIDWithIdentifiersGetter::new_k8s_instance_id_getter(
        k8s_store.clone(),
        Identifiers::default(),
    );

    // Creates Instance ID CM correctly tagged.
    let agent_instance_id = instance_id_getter.get(agent_id).unwrap();

    let mut config_loader = MockSuperAgentDynamicConfigLoaderMock::new();
    let config = format!(
        r#"
agents:
  {agent_id}:
    agent_type: {agent_fqn}
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
        agent_instance_id,
        instance_id_getter.get(agent_id).unwrap(),
        "Expects the Instance ID keeps the same since is get from the CM"
    );

    // Expect that the current_agent is removed on the second call.
    gc.collect().unwrap();
    block_on(api.get(resource_name)).expect_err("CR should be removed");
    assert_ne!(
        agent_instance_id,
        instance_id_getter.get(agent_id).unwrap(),
        "Expects the new Instance ID is generated after the CM removal"
    );
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_garbage_collector_with_missing_and_extra_kinds() {
    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    // Creates CRs labeled for two agents.
    let removed_agent_id = "removed";
    block_on(create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        removed_agent_id,
        Some(Labels::new(&AgentID::try_from(removed_agent_id.to_string()).unwrap()).get()),
        None,
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
            SyncK8sClient::try_new(
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
    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let sa_id = &AgentID::new_super_agent_id();
    block_on(create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        sa_id,
        Some(Labels::new(sa_id).get()),
        None,
    ));

    let k8s_client = Arc::new(
        SyncK8sClient::try_new(tokio_runtime(), test_ns.to_string(), vec![foo_type_meta()])
            .unwrap(),
    );
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

    let instance_id_getter = InstanceIDWithIdentifiersGetter::new_k8s_instance_id_getter(
        k8s_store.clone(),
        Identifiers::default(),
    );

    let sa_instance_id = instance_id_getter.get(sa_id).unwrap();

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
        sa_instance_id,
        instance_id_getter.get(sa_id).unwrap(),
        "Expects the Instance ID keeps the same since is get from the CM"
    );
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_garbage_collector_deletes_only_expected_resources() {
    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());
    let fqn = &AgentTypeFQN::try_from("ns/test:1.2.3").unwrap();
    let fqn_old = &AgentTypeFQN::try_from("ns/test:0.0.1").unwrap();
    let agent_id = &AgentID::new("agent-id").unwrap();
    let agent_id_unknonw = &AgentID::new("agent-id-unknown").unwrap();

    block_on(create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        "not-deleted",
        Some(Labels::new(agent_id).get()),
        Some(Annotations::new_agent_fqn_annotation(fqn).get()),
    ));

    block_on(create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        "sa-id",
        Some(Labels::new(&AgentID::new_super_agent_id()).get()),
        None,
    ));

    block_on(create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        "unmanaged-missing-labels",
        None,
        None,
    ));

    block_on(create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        "old-fqn",
        Some(Labels::new(agent_id).get()),
        Some(Annotations::new_agent_fqn_annotation(fqn_old).get()),
    ));

    block_on(create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        "id-unknown",
        Some(Labels::new(agent_id_unknonw).get()),
        Some(Annotations::new_agent_fqn_annotation(fqn).get()),
    ));

    let k8s_client = Arc::new(
        SyncK8sClient::try_new(tokio_runtime(), test_ns.to_string(), vec![foo_type_meta()])
            .unwrap(),
    );

    let mut config_loader = MockSuperAgentDynamicConfigLoaderMock::new();
    let config = format!(
        r#"
agents:
  {agent_id}:
    agent_type: {fqn}
"#
    );
    config_loader.expect_load().times(1).returning(move || {
        Ok(serde_yaml::from_str::<SuperAgentDynamicConfig>(config.as_str()).unwrap())
    });

    let mut gc = NotStartedK8sGarbageCollector::new(Arc::new(config_loader), k8s_client);

    // Expects the GC do not clean any resource related to the SA, running SubAgents or unmanaged resources.
    gc.collect().unwrap();
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);

    block_on(api.get("not-deleted")).expect("CR should exist");
    block_on(api.get("sa-id")).expect("CR should exist");
    block_on(api.get("unmanaged-missing-labels")).expect("CR should exist");

    block_on(api.get("old-fqn")).expect_err("CR should not exist");
    block_on(api.get("id-unknown")).expect_err("CR should not exist");
}
