use super::sub_agent::NotStartedSubAgentK8s;
use crate::config::super_agent_configs::{K8sConfig, SubAgentConfig};
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentEvent;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::build_opamp_and_start_client;
use crate::{
    config::super_agent_configs::AgentID,
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::k8s::supervisor::CRSupervisor,
    sub_agent::{error::SubAgentBuilderError, logger::AgentLog, SubAgentBuilder},
};
use kube::core::TypeMeta;
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::config::agent_type::runtime_config::K8sObject;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::super_agent::effective_agents_assembler::EffectiveAgentsAssembler;

pub struct K8sSubAgentBuilder<'a, C, O, I, A>
where
    C: Callbacks,
    O: OpAMPClientBuilder<C>,
    I: InstanceIDGetter,
    A: EffectiveAgentsAssembler,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,

    // Needed to include this in the struct to avoid the compiler complaining about not using the type parameter `C`.
    // It's actually used as a generic parameter for the `OpAMPClientBuilder` instance bound by type parameter `O`.
    // Feel free to remove this when the actual implementations (Callbacks instance for K8s agents) make it redundant!
    _callbacks: std::marker::PhantomData<C>,
    k8s_client: Arc<SyncK8sClient>,
    effective_agent_assembler: &'a A,
    k8s_config: K8sConfig,
}

impl<'a, C, O, I, A> K8sSubAgentBuilder<'a, C, O, I, A>
where
    C: Callbacks,
    O: OpAMPClientBuilder<C>,
    I: InstanceIDGetter,
    A: EffectiveAgentsAssembler,
{
    pub fn new(
        opamp_builder: Option<&'a O>,
        instance_id_getter: &'a I,
        k8s_client: Arc<SyncK8sClient>,
        effective_agent_assembler: &'a A,
        k8s_config: K8sConfig,
    ) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,
            _callbacks: std::marker::PhantomData,
            k8s_client,
            effective_agent_assembler,
            k8s_config,
        }
    }
}

impl<'a, C, O, I, A> SubAgentBuilder for K8sSubAgentBuilder<'a, C, O, I, A>
where
    C: Callbacks,
    O: OpAMPClientBuilder<C>,
    I: InstanceIDGetter,
    A: EffectiveAgentsAssembler,
{
    type NotStartedSubAgent = NotStartedSubAgentK8s<C, O::Client>;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        _tx: std::sync::mpsc::Sender<AgentLog>,
        _sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        let (sub_agent_opamp_publisher, _sub_agent_opamp_consumer) = pub_sub();

        let maybe_opamp_client = build_opamp_and_start_client(
            sub_agent_opamp_publisher,
            self.opamp_builder,
            self.instance_id_getter,
            agent_id.clone(),
            &sub_agent_config.agent_type,
            HashMap::from([(
                "cluster.name".to_string(),
                DescriptionValueType::String(self.k8s_config.cluster_name.to_string()),
            )]),
        )?;

        let effective_agent = self
            .effective_agent_assembler
            .assemble_agent(&agent_id, sub_agent_config)?;

        let k8s_objects = effective_agent
            .runtime_config
            .deployment
            .k8s
            .as_ref()
            .ok_or(SubAgentBuilderError::ConfigError(
                "Missing k8s deployment configuration".into(),
            ))?
            .objects
            .clone();

        // Validate Kubernetes objects against the list of supported resources.
        validate_k8s_objects(&k8s_objects, &self.k8s_config.cr_type_meta)?;

        // Clone the k8s_client on each build.
        let supervisor = CRSupervisor::new(agent_id.clone(), self.k8s_client.clone(), k8s_objects);

        Ok(NotStartedSubAgentK8s::new(
            agent_id,
            maybe_opamp_client,
            supervisor,
        ))
    }
}

fn validate_k8s_objects(
    objects: &HashMap<String, K8sObject>,
    supported_types: &[TypeMeta],
) -> Result<(), SubAgentBuilderError> {
    let supported_set: HashSet<(&str, &str)> = supported_types
        .iter()
        .map(|tm| (tm.api_version.as_str(), tm.kind.as_str()))
        .collect();

    for k8s_obj in objects.values() {
        let obj_key = (k8s_obj.api_version.as_str(), k8s_obj.kind.as_str());
        if !supported_set.contains(&obj_key) {
            return Err(SubAgentBuilderError::UnsupportedK8sObject(format!(
                "Unsupported Kubernetes object with api_version '{}' and kind '{}'",
                k8s_obj.api_version, k8s_obj.kind
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::config::agent_type::agent_types::FinalAgent;
    use crate::config::agent_type::runtime_config::K8s;
    use crate::config::super_agent_configs::K8sConfig;
    use crate::event::channel::pub_sub;
    use crate::k8s::error::K8sError;
    use crate::opamp::callbacks::tests::MockCallbacksMock;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::instance_id::getter::test::MockInstanceIDGetterMock;
    use crate::opamp::operations::start_settings;
    use crate::super_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::{
        k8s::client::MockSyncK8sClient,
        opamp::client_builder::test::MockOpAMPClientBuilderMock,
        sub_agent::{NotStartedSubAgent, StartedSubAgent},
    };
    use assert_matches::assert_matches;
    use opamp_client::operation::settings::DescriptionValueType;
    use std::{collections::HashMap, sync::mpsc::channel};

    #[test]
    fn build_start_stop() {
        // opamp builder mock
        let instance_id = "k8s-test-instance-id";
        let cluster_name = "test-cluster";
        let mut opamp_builder: MockOpAMPClientBuilderMock<MockCallbacksMock> =
            MockOpAMPClientBuilderMock::new();
        let final_agent = k8s_final_agent(true);
        let sub_agent_config = SubAgentConfig {
            agent_type: final_agent.agent_type(),
        };
        let start_settings = start_settings(
            instance_id.to_string(),
            &sub_agent_config.agent_type,
            HashMap::from([(
                "cluster.name".to_string(),
                DescriptionValueType::String(cluster_name.to_string()),
            )]),
        );

        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);
        started_client.should_stop(1);

        opamp_builder.should_build_and_start(
            AgentID::new("k8s-test").unwrap(),
            start_settings,
            started_client,
        );
        // instance id getter mock
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            &AgentID::new("k8s-test").unwrap(),
            "k8s-test-instance-id".to_string(),
        );

        // instance K8s client mock
        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_apply_dynamic_object_if_changed()
            .times(1)
            .returning(|_| Ok(()));
        mock_client
            .expect_default_namespace()
            .return_const("default".to_string());

        let sub_agent_id = AgentID::new("k8s-test").unwrap();

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler.should_assemble_agent(
            &sub_agent_id,
            &sub_agent_config,
            final_agent,
        );

        let k8s_config = K8sConfig {
            cluster_name: cluster_name.to_string(),
            namespace: "test-namespace".to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };

        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(mock_client),
            &effective_agent_assembler,
            k8s_config,
        );

        let (tx, _) = channel();
        let (super_agent_publisher, _super_agent_consumer) = pub_sub();
        let started_agent = builder
            .build(
                AgentID::new("k8s-test").unwrap(),
                &sub_agent_config,
                tx,
                super_agent_publisher,
            )
            .unwrap() // Not started agent
            .run()
            .unwrap();
        assert!(started_agent.stop().is_ok())
    }

    #[test]
    fn build_start_fails() {
        let test_issue = "random issue";
        let cluster_name = "test-cluster";
        let reported_message = "Supervisor run error: `the kube client returned an error: `while getting dynamic resource: random issue``";

        // opamp builder mock
        let instance_id = "k8s-test-instance-id";
        let mut opamp_builder: MockOpAMPClientBuilderMock<MockCallbacksMock> =
            MockOpAMPClientBuilderMock::new();
        let final_agent = k8s_final_agent(true);
        let sub_agent_config = SubAgentConfig {
            agent_type: final_agent.agent_type().clone(),
        };
        let start_settings = start_settings(
            instance_id.to_string(),
            &sub_agent_config.agent_type,
            HashMap::from([(
                "cluster.name".to_string(),
                DescriptionValueType::String(cluster_name.to_string()),
            )]),
        );

        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_specific_health(
            1,
            opamp_client::opamp::proto::AgentHealth {
                healthy: false,
                last_error: reported_message.to_string(),
                start_time_unix_nano: 0,
            },
        );
        started_client.should_stop(1);

        opamp_builder.should_build_and_start(
            AgentID::new("k8s-test").unwrap(),
            start_settings,
            started_client,
        );
        // instance id getter mock
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            &AgentID::new("k8s-test").unwrap(),
            "k8s-test-instance-id".to_string(),
        );

        // instance K8s client mock now FAILING to apply
        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_apply_dynamic_object_if_changed()
            .times(1)
            .returning(|_| Err(K8sError::GetDynamic(test_issue.to_string())));
        mock_client
            .expect_default_namespace()
            .return_const("default".to_string());

        let sub_agent_id = AgentID::new("k8s-test").unwrap();

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler.should_assemble_agent(
            &sub_agent_id,
            &sub_agent_config,
            final_agent,
        );

        let k8s_config = K8sConfig {
            cluster_name: "test-cluster".to_string(),
            namespace: "test-namespace".to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };

        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(mock_client),
            &effective_agent_assembler,
            k8s_config,
        );

        let (tx, _) = channel();
        let (super_agent_publisher, _super_agent_consumer) = pub_sub();
        assert!(builder
            .build(
                AgentID::new("k8s-test").unwrap(),
                &sub_agent_config,
                tx,
                super_agent_publisher,
            )
            .unwrap() // Not started agent
            .run()
            .is_err())
    }

    #[test]
    fn build_error_with_invalid_object_kind() {
        let instance_id = "k8s-test-instance-id";
        let cluster_name = "cluster-name";

        let mut opamp_builder: MockOpAMPClientBuilderMock<MockCallbacksMock> =
            MockOpAMPClientBuilderMock::new();
        let final_agent = k8s_final_agent(false); // false indicates invalid kind
        let sub_agent_config = SubAgentConfig {
            agent_type: final_agent.agent_type().clone(),
        };
        let start_settings = start_settings(
            instance_id.to_string(),
            &sub_agent_config.agent_type,
            HashMap::from([(
                "cluster.name".to_string(),
                DescriptionValueType::String(cluster_name.to_string()),
            )]),
        );
        opamp_builder.should_build_and_start(
            AgentID::new("k8s-test").unwrap(),
            start_settings,
            MockStartedOpAMPClientMock::new(),
        );
        // instance id getter mock
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            &AgentID::new("k8s-test").unwrap(),
            "k8s-test-instance-id".to_string(),
        );

        let sub_agent_id = AgentID::new("k8s-test").unwrap();
        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler.should_assemble_agent(
            &sub_agent_id,
            &sub_agent_config,
            final_agent,
        );

        let k8s_config = K8sConfig {
            cluster_name: cluster_name.to_string(),
            namespace: "test-namespace".to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };

        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(MockSyncK8sClient::default()),
            &effective_agent_assembler,
            k8s_config,
        );

        let (tx, _) = channel();
        let (opamp_publisher, _opamp_consumer) = pub_sub();
        let build_result = builder.build(sub_agent_id, &sub_agent_config, tx, opamp_publisher);

        let error = build_result.err().expect("Expected an error");

        assert_matches!(error, SubAgentBuilderError::UnsupportedK8sObject(_));
    }

    fn k8s_final_agent(valid_kind: bool) -> FinalAgent {
        let kind = if valid_kind {
            "HelmRepository".to_string()
        } else {
            "UnsupportedKind".to_string()
        };

        let k8s_object = K8sObject {
            api_version: "source.toolkit.fluxcd.io/v1beta2".to_string(),
            kind,
            ..Default::default()
        };

        let mut objects = HashMap::new();
        objects.insert("sample_object".to_string(), k8s_object);

        let mut final_agent: FinalAgent = FinalAgent::default();
        final_agent.runtime_config.deployment.k8s = Some(K8s { objects });
        final_agent
    }
}
