use crate::common::{
    retry::retry,
    runtime::{block_on, tokio_runtime},
};

use super::tools::{
    k8s_env::K8sEnv,
    test_crd::{create_foo_cr, foo_type_meta, Foo},
};
use k8s_openapi::api::core::v1::Secret;
use kube::{api::Api, core::TypeMeta};
use mockall::mock;
use newrelic_agent_control::sub_agent::k8s::supervisor::NotStartedSupervisorK8s;
use newrelic_agent_control::{
    agent_control::config::default_group_version_kinds, agent_type::agent_type_id::AgentTypeID,
};
use newrelic_agent_control::{
    agent_control::resource_cleaner::k8s_garbage_collector::K8sGarbageCollector,
    opamp::instance_id::k8s::getter::Identifiers,
};
use newrelic_agent_control::{
    agent_control::{agent_id::AgentID, defaults::AGENT_CONTROL_ID},
    k8s::{client::SyncK8sClient, store::K8sStore},
    opamp::instance_id::getter::{InstanceIDGetter, InstanceIDWithIdentifiersGetter},
};
use newrelic_agent_control::{
    agent_control::{
        config::{AgentControlConfig, AgentControlConfigError, AgentControlDynamicConfig},
        config_storer::loader_storer::{AgentControlConfigLoader, AgentControlDynamicConfigLoader},
    },
    k8s::labels::Labels,
};
use newrelic_agent_control::{
    agent_type::runtime_config::k8s::K8s, sub_agent::identity::AgentIdentity,
};
use newrelic_agent_control::{
    agent_type::runtime_config::k8s::K8sObject, k8s::annotations::Annotations,
};
use std::{collections::HashMap, sync::Arc, time::Duration};

// Setup AgentControlConfigLoader mock
mock! {
    pub AgentControlConfigLoader {}

    impl AgentControlConfigLoader for AgentControlConfigLoader {
        fn load(&self) -> Result<AgentControlConfig, AgentControlConfigError>;
    }
}

// Setup AgentControlDynamicConfigLoader mock
mock! {
    pub AgentControlDynamicConfigLoader{}

    impl AgentControlDynamicConfigLoader for AgentControlDynamicConfigLoader {
        fn load(&self) -> Result<AgentControlDynamicConfig, AgentControlConfigError>;
    }
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_garbage_collector_cleans_removed_agent_resources() {
    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let agent_identity = AgentIdentity::from((
        AgentID::new("sub-agent").unwrap(),
        AgentTypeID::try_from("ns/test:1.2.3").unwrap(),
    ));

    let k8s_client =
        Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.to_string()).unwrap());

    let resource_name = "test-different-from-agent-id";
    let secret_name = "test-secret-name";

    let s = NotStartedSupervisorK8s::new(
        agent_identity.clone(),
        k8s_client.clone(),
        K8s {
            objects: HashMap::from([
                (
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
                ),
                (
                    "fooSecret".to_string(),
                    serde_yaml::from_str::<K8sObject>(
                        format!(
                            r#"
    apiVersion: {}
    kind: {}
    stringData:
      values.yaml: |
        nameOverride: "override-by-secret"
    metadata:
      name: {}
            "#,
                            "v1", "Secret", secret_name,
                        )
                        .as_str(),
                    )
                    .unwrap(),
                ),
            ]),
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
    let agent_instance_id = instance_id_getter.get(&agent_identity.id).unwrap();

    let config = format!(
        r#"
agents:
  {}:
    agent_type: {}
"#,
        agent_identity.id, agent_identity.agent_type_id
    );

    let gc = K8sGarbageCollector {
        k8s_client,
        cr_type_meta: vec![
            foo_type_meta(),
            TypeMeta {
                api_version: "v1".to_string(),
                kind: "Secret".to_string(),
            },
        ],
    };

    // Expects the GC to keep the agent cr and secret from the config, event if looking for multiple kinds or that
    // are missing in the cluster.
    let first_agents_config = serde_yaml::from_str::<AgentControlDynamicConfig>(config.as_str())
        .unwrap()
        .agents;
    gc.retain(K8sGarbageCollector::active_config_ids(&first_agents_config))
        .unwrap();
    let api_foo: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    block_on(api_foo.get(resource_name)).expect("CR should exist");
    let api_secret: Api<Secret> = Api::namespaced(test.client.clone(), &test_ns);
    block_on(api_secret.get(secret_name)).expect("Secret should exist");
    assert_eq!(
        agent_instance_id,
        instance_id_getter.get(&agent_identity.id).unwrap(),
        "Expects the Instance ID keeps the same since is get from the CM"
    );

    // Expect that the current_agent and secret to be removed on the second call.
    let second_agents_config = serde_yaml::from_str::<AgentControlDynamicConfig>("agents: {}")
        .unwrap()
        .agents;
    gc.retain(K8sGarbageCollector::active_config_ids(
        &second_agents_config,
    ))
    .unwrap();
    retry(60, Duration::from_secs(1), || {
        if block_on(api_foo.get(resource_name)).is_ok() {
            return Err("CR should be removed".into());
        };
        if block_on(api_secret.get(secret_name)).is_ok() {
            return Err("Secret should be removed".into());
        };
        Ok(())
    });
    assert_ne!(
        agent_instance_id,
        instance_id_getter.get(&agent_identity.id).unwrap(),
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

    // This kind is not present in the cluster.
    let missing_kind = TypeMeta {
        api_version: "missing.com/v1".to_string(),
        kind: "Missing".to_string(),
    };

    let gc = K8sGarbageCollector {
        k8s_client: Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.to_string()).unwrap()),
        cr_type_meta: vec![missing_kind, foo_type_meta()],
    };

    let agents_config = serde_yaml::from_str::<AgentControlDynamicConfig>("agents: {}")
        .unwrap()
        .agents;
    // Expects the GC to clean the "removed" agent CR.
    gc.retain(K8sGarbageCollector::active_config_ids(&agents_config))
        .unwrap();
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    block_on(api.get(removed_agent_id)).expect_err("fail garbage collecting removed agent");
}

#[test]
#[ignore = "needs k8s cluster"]
fn k8s_garbage_collector_does_not_remove_agent_control() {
    let mut test = block_on(K8sEnv::new());
    let test_ns = block_on(test.test_namespace());

    let sa_id = &AgentID::new_agent_control_id();
    block_on(create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        sa_id,
        Some(Labels::new(sa_id).get()),
        None,
    ));

    let k8s_client =
        Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.to_string()).unwrap());
    let k8s_store = Arc::new(K8sStore::new(k8s_client.clone()));

    let instance_id_getter = InstanceIDWithIdentifiersGetter::new_k8s_instance_id_getter(
        k8s_store.clone(),
        Identifiers::default(),
    );

    let sa_instance_id = instance_id_getter.get(sa_id).unwrap();

    let gc = K8sGarbageCollector {
        k8s_client,
        cr_type_meta: default_group_version_kinds(),
    };

    // Expects the GC do not clean any resource related to the SA.
    let agents_config = serde_yaml::from_str::<AgentControlDynamicConfig>("agents: {}")
        .unwrap()
        .agents;
    gc.retain(K8sGarbageCollector::active_config_ids(&agents_config))
        .unwrap();
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);
    block_on(api.get(AGENT_CONTROL_ID)).expect("CR should exist");
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
    let fqn = &AgentTypeID::try_from("ns/test:1.2.3").unwrap();
    let fqn_old = &AgentTypeID::try_from("ns/test:0.0.1").unwrap();
    let agent_id = &AgentID::new("agent-id").unwrap();
    let agent_id_unknonw = &AgentID::new("agent-id-unknown").unwrap();

    block_on(create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        "not-deleted",
        Some(Labels::new(agent_id).get()),
        Some(Annotations::new_agent_type_id_annotation(fqn).get()),
    ));

    block_on(create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        "sa-id",
        Some(Labels::new(&AgentID::new_agent_control_id()).get()),
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
        Some(Annotations::new_agent_type_id_annotation(fqn_old).get()),
    ));

    block_on(create_foo_cr(
        test.client.to_owned(),
        test_ns.as_str(),
        "id-unknown",
        Some(Labels::new(agent_id_unknonw).get()),
        Some(Annotations::new_agent_type_id_annotation(fqn).get()),
    ));

    let config = format!(
        r#"
agents:
  {agent_id}:
    agent_type: {fqn}
"#
    );

    let gc = K8sGarbageCollector {
        k8s_client: Arc::new(SyncK8sClient::try_new(tokio_runtime(), test_ns.to_string()).unwrap()),
        cr_type_meta: vec![foo_type_meta()],
    };

    // Expects the GC do not clean any resource related to the SA, running SubAgents or unmanaged resources.
    let agents_config = serde_yaml::from_str::<AgentControlDynamicConfig>(config.as_str())
        .unwrap()
        .agents;
    gc.retain(K8sGarbageCollector::active_config_ids(&agents_config))
        .unwrap();
    let api: Api<Foo> = Api::namespaced(test.client.clone(), &test_ns);

    block_on(api.get("not-deleted")).expect("CR should exist");
    block_on(api.get("sa-id")).expect("CR should exist");
    block_on(api.get("unmanaged-missing-labels")).expect("CR should exist");

    block_on(api.get("old-fqn")).expect_err("CR should not exist");
    block_on(api.get("id-unknown")).expect_err("CR should not exist");
}
