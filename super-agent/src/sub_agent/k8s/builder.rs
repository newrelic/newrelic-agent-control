use super::sub_agent::SubAgentK8s;
use crate::agent_type::environment::Environment;
use crate::agent_type::runtime_config::K8sObject;
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentEvent;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::build_opamp_and_start_client;
use crate::opamp::remote_config_report::{
    report_remote_config_status_applied, report_remote_config_status_error,
};
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use crate::sub_agent::event_processor_builder::SubAgentEventProcessorBuilder;
use crate::sub_agent::{NotStarted, SubAgentCallbacks};
use crate::super_agent::config::{AgentID, K8sConfig, SubAgentConfig};
use crate::{
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::k8s::supervisor::CRSupervisor,
    sub_agent::{error::SubAgentBuilderError, SubAgentBuilder},
};
use kube::core::TypeMeta;
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, error, warn};

pub struct K8sSubAgentBuilder<'a, O, I, HR, A, E>
where
    O: OpAMPClientBuilder<SubAgentCallbacks>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client>,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,
    hash_repository: Arc<HR>,
    k8s_client: Arc<SyncK8sClient>,
    effective_agent_assembler: &'a A,
    event_processor_builder: &'a E,
    k8s_config: K8sConfig,
}

impl<'a, O, I, HR, A, E> K8sSubAgentBuilder<'a, O, I, HR, A, E>
where
    O: OpAMPClientBuilder<SubAgentCallbacks>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client>,
{
    pub fn new(
        opamp_builder: Option<&'a O>,
        instance_id_getter: &'a I,
        k8s_client: Arc<SyncK8sClient>,
        hash_repository: Arc<HR>,
        effective_agent_assembler: &'a A,
        event_processor_builder: &'a E,
        k8s_config: K8sConfig,
    ) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,
            hash_repository,
            k8s_client,
            effective_agent_assembler,
            event_processor_builder,
            k8s_config,
        }
    }
}

impl<'a, O, I, HR, A, E> SubAgentBuilder for K8sSubAgentBuilder<'a, O, I, HR, A, E>
where
    O: OpAMPClientBuilder<SubAgentCallbacks>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client>,
{
    type NotStartedSubAgent = SubAgentK8s<NotStarted<E::SubAgentEventProcessor>>;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        let (sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();

        debug!("Building subAgent {}", agent_id);

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

        let effective_agent_res = self.effective_agent_assembler.assemble_agent(
            &agent_id,
            sub_agent_config,
            &Environment::K8s,
        );

        let mut has_supervisors = true;

        // TODO the logic here is 100% similar to the onhost one, we should review and possibly merge them
        if let Some(opamp_client) = &maybe_opamp_client {
            match self.hash_repository.get(&agent_id) {
                Err(e) => error!("hash repository error: {}", e),
                Ok(None) => warn!("hash repository not found for agent: {}", &agent_id),
                Ok(Some(mut hash)) => {
                    if let Err(err) = effective_agent_res.as_ref() {
                        report_remote_config_status_error(opamp_client, &hash, err.to_string())?;
                        error!(
                            "Failed to assemble agent {} and to create supervisors, only the opamp client will be listening for a fixed configuration",
                            agent_id
                        );
                        // report the failed status for remote config and let the opamp client
                        // running with no supervisors so the configuration can be fixed
                        has_supervisors = false;
                    } else if hash.is_applying() {
                        report_remote_config_status_applied(opamp_client, &hash)?;
                        hash.apply();
                        self.hash_repository.save(&agent_id, &hash)?;
                    } else if hash.is_failed() {
                        // failed hash always has the error message
                        let error_message = hash.error_message().unwrap();
                        report_remote_config_status_error(
                            opamp_client,
                            &hash,
                            error_message.to_string(),
                        )?;
                    }
                }
            }
        }

        // When running with no CRSupervisor only the opamp server is enabled.
        // This behaviour is needed to allow a subAgent to download a fixed configuration.
        let supervisor: Option<CRSupervisor> = match has_supervisors {
            false => None,
            true => Some(build_cr_supervisors(
                &agent_id,
                effective_agent_res?,
                self.k8s_client.clone(),
                &self.k8s_config,
            )?),
        };

        let event_processor = self.event_processor_builder.build(
            agent_id.clone(),
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            maybe_opamp_client,
        );

        Ok(SubAgentK8s::new(
            agent_id,
            event_processor,
            sub_agent_internal_publisher,
            supervisor,
        ))
    }
}

fn build_cr_supervisors(
    agent_id: &AgentID,
    effective_agent: EffectiveAgent,
    k8s_client: Arc<SyncK8sClient>,
    k8s_config: &K8sConfig,
) -> Result<CRSupervisor, SubAgentBuilderError> {
    debug!("Building CR supervisors {}", agent_id);

    let k8s_objects = effective_agent
        .get_runtime_config()
        .deployment
        .k8s
        .as_ref()
        .ok_or(SubAgentBuilderError::ConfigError(
            "Missing k8s deployment configuration".into(),
        ))?
        .objects
        .clone();

    // Validate Kubernetes objects against the list of supported resources.
    validate_k8s_objects(&k8s_objects, &k8s_config.cr_type_meta)?;

    // Clone the k8s_client on each build.
    Ok(CRSupervisor::new(agent_id.clone(), k8s_client, k8s_objects))
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
    use crate::agent_type::agent_metadata::AgentMetadata;
    use crate::agent_type::runtime_config::{Deployment, K8s, Runtime};
    use crate::event::channel::pub_sub;
    use crate::k8s::error::K8sError;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::instance_id::getter::test::MockInstanceIDGetterMock;
    use crate::opamp::operations::start_settings;
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::event_processor_builder::test::MockSubAgentEventProcessorBuilderMock;
    use crate::{
        k8s::client::MockSyncK8sClient,
        opamp::client_builder::test::MockOpAMPClientBuilderMock,
        sub_agent::{NotStartedSubAgent, StartedSubAgent},
    };
    use assert_matches::assert_matches;
    use opamp_client::operation::settings::DescriptionValueType;
    use std::collections::HashMap;

    #[test]
    fn build_start_stop() {
        // opamp builder mock
        let instance_id = "k8s-test-instance-id";
        let cluster_name = "test-cluster";
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();

        let effective_agent = k8s_effective_agent(AgentID::new("k8s-test").unwrap(), true);
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentMetadata::default().to_string().as_str().into(),
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
        started_client.should_set_any_remote_config_status(1);

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
            &Environment::K8s,
            effective_agent,
        );

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let hash = Hash::new("a-hash".to_string());
            Ok(Some(hash))
        });
        hash_repository_mock
            .expect_save()
            .times(1)
            .returning(|_, _| Ok(()));

        let mut sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();
        sub_agent_event_processor_builder.should_return_event_processor_with_consumer();

        let k8s_config = K8sConfig {
            cluster_name: cluster_name.to_string(),
            namespace: "test-namespace".to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };

        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(mock_client),
            Arc::new(hash_repository_mock),
            &effective_agent_assembler,
            &sub_agent_event_processor_builder,
            k8s_config,
        );

        let (super_agent_publisher, _super_agent_consumer) = pub_sub();
        let started_agent = builder
            .build(
                AgentID::new("k8s-test").unwrap(),
                &sub_agent_config,
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

        // opamp builder mock
        let instance_id = "k8s-test-instance-id";
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let effective_agent = k8s_effective_agent(AgentID::new("k8s-test").unwrap(), true);
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentMetadata::default().to_string().as_str().into(),
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
        started_client.should_set_any_remote_config_status(1);

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
            &Environment::K8s,
            effective_agent,
        );

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let hash = Hash::new("a-hash".to_string());
            Ok(Some(hash))
        });
        hash_repository_mock
            .expect_save()
            .times(1)
            .returning(|_, _| Ok(()));

        let mut sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();
        let sub_agent_event_processor = MockEventProcessorMock::default();
        sub_agent_event_processor_builder.should_build(sub_agent_event_processor);

        let k8s_config = K8sConfig {
            cluster_name: "test-cluster".to_string(),
            namespace: "test-namespace".to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };

        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(mock_client),
            Arc::new(hash_repository_mock),
            &effective_agent_assembler,
            &sub_agent_event_processor_builder,
            k8s_config,
        );

        let (super_agent_publisher, _super_agent_consumer) = pub_sub();
        assert!(builder
            .build(
                AgentID::new("k8s-test").unwrap(),
                &sub_agent_config,
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

        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let effective_agent = k8s_effective_agent(AgentID::new("k8s-test").unwrap(), false);
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentMetadata::default().to_string().as_str().into(),
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
        started_client.should_set_any_remote_config_status(1);

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

        let sub_agent_id = AgentID::new("k8s-test").unwrap();
        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler.should_assemble_agent(
            &sub_agent_id,
            &sub_agent_config,
            &Environment::K8s,
            effective_agent,
        );

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let hash = Hash::new("a-hash".to_string());
            Ok(Some(hash))
        });
        hash_repository_mock
            .expect_save()
            .times(1)
            .returning(|_, _| Ok(()));

        let mut sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();
        sub_agent_event_processor_builder.expect_build().never();

        let k8s_config = K8sConfig {
            cluster_name: cluster_name.to_string(),
            namespace: "test-namespace".to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };

        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(MockSyncK8sClient::default()),
            Arc::new(hash_repository_mock),
            &effective_agent_assembler,
            &sub_agent_event_processor_builder,
            k8s_config,
        );

        let (opamp_publisher, _opamp_consumer) = pub_sub();
        let build_result = builder.build(sub_agent_id, &sub_agent_config, opamp_publisher);

        let error = build_result.err().expect("Expected an error");

        assert_matches!(error, SubAgentBuilderError::UnsupportedK8sObject(_));
    }

    fn k8s_effective_agent(agent_id: AgentID, valid_kind: bool) -> EffectiveAgent {
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

        EffectiveAgent::new(
            agent_id,
            Runtime {
                deployment: Deployment {
                    on_host: None,
                    k8s: Some(K8s { objects }),
                },
            },
        )
    }
}
