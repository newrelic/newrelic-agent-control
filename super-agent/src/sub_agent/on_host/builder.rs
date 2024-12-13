use crate::agent_type::environment::Environment;
use crate::context::Context;
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentEvent;
use crate::opamp::effective_config::loader::EffectiveConfigLoader;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::build_sub_agent_opamp;
use crate::sub_agent::config_validator::ConfigValidator;
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use crate::sub_agent::event_handler::opamp::remote_config_handler::RemoteConfigHandler;
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use crate::sub_agent::on_host::supervisor::NotStartedSupervisorOnHost;
use crate::sub_agent::supervisor::assembler::SupervisorAssembler;
use crate::sub_agent::supervisor::builder::SupervisorBuilder;
use crate::sub_agent::SubAgent;
use crate::sub_agent::SubAgentCallbacks;
use crate::super_agent::config::{AgentID, SubAgentConfig};
use crate::super_agent::defaults::{
    sub_agent_version, HOST_NAME_ATTRIBUTE_KEY, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
    OPAMP_SERVICE_VERSION,
};
use crate::values::yaml_config_repository::YAMLConfigRepository;
use crate::{
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::{error::SubAgentBuilderError, SubAgentBuilder},
};
#[cfg(unix)]
use nix::unistd::gethostname;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;

pub struct OnHostSubAgentBuilder<'a, O, I, HR, A, G, Y>
where
    G: EffectiveConfigLoader,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    Y: YAMLConfigRepository,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,
    hash_repository: Arc<HR>,
    effective_agent_assembler: Arc<A>,
    logging_path: PathBuf,
    yaml_config_repository: Arc<Y>,

    // This is needed to ensure the generic type parameter G is used in the struct.
    // Else Rust will reject this, complaining that the type parameter is not used.
    _effective_config_loader: PhantomData<G>,
}

impl<'a, O, I, HR, A, G, Y> OnHostSubAgentBuilder<'a, O, I, HR, A, G, Y>
where
    G: EffectiveConfigLoader,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    Y: YAMLConfigRepository,
{
    pub fn new(
        opamp_builder: Option<&'a O>,
        instance_id_getter: &'a I,
        hash_repository: Arc<HR>,
        effective_agent_assembler: Arc<A>,
        logging_path: PathBuf,
        yaml_config_repository: Arc<Y>,
    ) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,
            hash_repository,
            effective_agent_assembler,
            logging_path,
            yaml_config_repository,

            _effective_config_loader: PhantomData,
        }
    }
}

impl<O, I, HR, A, G, Y> SubAgentBuilder for OnHostSubAgentBuilder<'_, O, I, HR, A, G, Y>
where
    G: EffectiveConfigLoader + Send + Sync + 'static,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>> + Send + Sync + 'static,
    I: InstanceIDGetter,
    HR: HashRepository + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
    Y: YAMLConfigRepository,
{
    type NotStartedSubAgent =
        SubAgent<O::Client, SubAgentCallbacks<G>, A, SupervisortBuilderOnHost, HR, Y>;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        let mut identifying_attributes = HashMap::from([(
            OPAMP_SERVICE_VERSION.to_string(),
            sub_agent_config.agent_type.version().into(),
        )]);
        if let Some(agent_version) = sub_agent_version(sub_agent_config.agent_type.name().as_str())
        {
            identifying_attributes.insert(
                OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
                agent_version.clone(),
            );
        }
        let (maybe_opamp_client, sub_agent_opamp_consumer) = self
            .opamp_builder
            .map(|builder| {
                build_sub_agent_opamp(
                    builder,
                    self.instance_id_getter,
                    agent_id.clone(),
                    &sub_agent_config.agent_type,
                    identifying_attributes,
                    HashMap::from([(HOST_NAME_ATTRIBUTE_KEY.to_string(), get_hostname().into())]),
                )
            })
            // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
            .transpose()?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();

        let remote_config_handler = RemoteConfigHandler::new(
            Arc::new(
                ConfigValidator::try_new().expect("Failed to compile config validation regexes"),
            ),
            agent_id.clone(),
            sub_agent_config.clone(),
            self.hash_repository.clone(),
            self.yaml_config_repository.clone(),
        );

        let supervisor_assembler = SupervisorAssembler::new(
            self.hash_repository.clone(),
            SupervisortBuilderOnHost::new(self.logging_path.clone()),
            agent_id.clone(),
            sub_agent_config.clone(),
            self.effective_agent_assembler.clone(),
            Environment::OnHost,
        );

        Ok(SubAgent::new(
            agent_id,
            sub_agent_config.clone(),
            maybe_opamp_client,
            supervisor_assembler,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            pub_sub(),
            remote_config_handler,
        ))
    }
}

fn get_hostname() -> String {
    #[cfg(unix)]
    return gethostname().unwrap_or_default().into_string().unwrap();

    #[cfg(not(unix))]
    return unimplemented!();
}

pub struct SupervisortBuilderOnHost {
    logging_path: PathBuf,
}

impl SupervisortBuilderOnHost {
    pub fn new(logging_path: PathBuf) -> Self {
        Self { logging_path }
    }
}

impl SupervisorBuilder for SupervisortBuilderOnHost {
    type SupervisorStarter = NotStartedSupervisorOnHost;

    fn build_supervisor(
        &self,
        effective_agent: EffectiveAgent,
    ) -> Result<Self::SupervisorStarter, SubAgentBuilderError> {
        let agent_id = effective_agent.get_agent_id().clone();
        let on_host = effective_agent.get_onhost_config()?.clone();

        let enable_file_logging = on_host.enable_file_logging.get();

        let maybe_exec = on_host.executable.map(|e| {
            ExecutableData::new(e.path.get())
                .with_args(e.args.get().into_vector())
                .with_env(e.env.get())
                .with_restart_policy(e.restart_policy.into())
        });

        let executable_supervisors =
            NotStartedSupervisorOnHost::new(agent_id, maybe_exec, Context::new(), on_host.health)
                .with_file_logging(enable_file_logging, self.logging_path.to_path_buf());

        Ok(executable_supervisors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::environment::Environment;
    use crate::agent_type::runtime_config::{Deployment, OnHost, Runtime};
    use crate::event::channel::pub_sub;
    use crate::opamp::client_builder::tests::MockOpAMPClientBuilderMock;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::hash_repository::repository::tests::MockHashRepositoryMock;
    use crate::opamp::instance_id::getter::tests::MockInstanceIDGetterMock;
    use crate::opamp::instance_id::InstanceID;
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::AgentTypeFQN;
    use crate::super_agent::defaults::{
        default_capabilities, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY, OPAMP_SERVICE_NAME,
        OPAMP_SERVICE_NAMESPACE, OPAMP_SERVICE_VERSION, PARENT_AGENT_ID_ATTRIBUTE_KEY,
    };
    use crate::values::yaml_config_repository::tests::MockYAMLConfigRepositoryMock;
    use nix::unistd::gethostname;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Failed;
    use opamp_client::operation::settings::{
        AgentDescription, DescriptionValueType, StartSettings,
    };
    use std::collections::HashMap;

    // TODO: tests below are testing not only the builder but also the sub-agent start/stop behavior.
    // We should re-consider their scope.
    #[test]
    fn build_start_stop() {
        let (opamp_publisher, _opamp_consumer) = pub_sub();
        let mut opamp_builder: MockOpAMPClientBuilderMock<
            SubAgentCallbacks<MockEffectiveConfigLoaderMock>,
        > = MockOpAMPClientBuilderMock::new();
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure:0.0.2")
                .unwrap(),
        };

        let sub_agent_instance_id = InstanceID::create();
        let super_agent_instance_id = InstanceID::create();

        let start_settings_infra = infra_agent_default_start_settings(
            &hostname,
            super_agent_instance_id.clone(),
            sub_agent_instance_id.clone(),
            &sub_agent_config,
        );

        let super_agent_id = AgentID::new_super_agent_id();
        let sub_agent_id = AgentID::new("infra-agent").unwrap();
        let final_agent =
            on_host_final_agent(sub_agent_id.clone(), sub_agent_config.agent_type.clone());

        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_any_remote_config_status(1);
        started_client.should_update_effective_config(2);
        started_client.should_stop(1);

        // Infra Agent OpAMP no final stop nor health, just after stopping on reload
        opamp_builder.should_build_and_start(
            sub_agent_id.clone(),
            start_settings_infra,
            started_client,
        );

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(&sub_agent_id, sub_agent_instance_id.clone());
        instance_id_getter.should_get(&super_agent_id, super_agent_instance_id.clone());

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let hash = Hash::new("a-hash".to_string());
            Ok(Some(hash))
        });
        hash_repository_mock
            .expect_save()
            .times(1)
            .returning(|_, _| Ok(()));

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler.should_assemble_agent(
            &sub_agent_id,
            &sub_agent_config,
            &Environment::OnHost,
            final_agent,
            1,
        );

        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        let on_host_builder = OnHostSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(hash_repository_mock),
            Arc::new(effective_agent_assembler),
            PathBuf::default(),
            Arc::new(remote_values_repo),
        );

        on_host_builder
            .build(sub_agent_id, &sub_agent_config, opamp_publisher)
            .unwrap()
            .run()
            .stop()
    }

    #[test]
    fn test_subagent_should_report_failed_config() {
        let (opamp_publisher, _opamp_consumer) = pub_sub();
        // Mocks
        let mut opamp_builder: MockOpAMPClientBuilderMock<
            SubAgentCallbacks<MockEffectiveConfigLoaderMock>,
        > = MockOpAMPClientBuilderMock::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();

        // Structures
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure:0.0.2")
                .unwrap(),
        };
        let sub_agent_instance_id = InstanceID::create();
        let super_agent_instance_id = InstanceID::create();

        let start_settings_infra = infra_agent_default_start_settings(
            &hostname,
            super_agent_instance_id.clone(),
            sub_agent_instance_id.clone(),
            &sub_agent_config,
        );

        let super_agent_id = AgentID::new_super_agent_id();
        let sub_agent_id = AgentID::new("infra-agent").unwrap();
        let final_agent =
            on_host_final_agent(sub_agent_id.clone(), sub_agent_config.agent_type.clone());
        // Expectations
        // Infra Agent OpAMP no final stop nor health, just after stopping on reload
        instance_id_getter.should_get(&sub_agent_id, sub_agent_instance_id.clone());
        instance_id_getter.should_get(&super_agent_id, super_agent_instance_id.clone());

        let mut started_client = MockStartedOpAMPClientMock::new();
        // failed conf should be reported
        started_client.should_set_remote_config_status(RemoteConfigStatus {
            error_message: "this is an error message".to_string(),
            status: Failed as i32,
            last_remote_config_hash: "a-hash".as_bytes().to_vec(),
        });

        opamp_builder.should_build_and_start(
            sub_agent_id.clone(),
            start_settings_infra,
            started_client,
        );

        effective_agent_assembler.should_assemble_agent(
            &sub_agent_id,
            &sub_agent_config,
            &Environment::OnHost,
            final_agent,
            1,
        );

        // return a failed hash
        let failed_hash =
            Hash::failed("a-hash".to_string(), "this is an error message".to_string());
        hash_repository_mock.should_get_hash(&sub_agent_id, failed_hash);

        let remote_values_repo = MockYAMLConfigRepositoryMock::default();

        // Sub Agent Builder
        let on_host_builder = OnHostSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(hash_repository_mock),
            Arc::new(effective_agent_assembler),
            PathBuf::default(),
            Arc::new(remote_values_repo),
        );

        let sub_agent = on_host_builder
            .build(sub_agent_id, &sub_agent_config, opamp_publisher)
            .expect("Subagent build should be OK");
        let _ = sub_agent.run(); // Running the sub-agent should report the failed configuration.
    }

    // HELPERS
    #[cfg(test)]
    fn on_host_final_agent(agent_id: AgentID, agent_fqn: AgentTypeFQN) -> EffectiveAgent {
        use crate::agent_type::definition::TemplateableValue;

        EffectiveAgent::new(
            agent_id,
            agent_fqn,
            Runtime {
                deployment: Deployment {
                    on_host: Some(OnHost {
                        executable: None,
                        enable_file_logging: TemplateableValue::new(false),
                        health: None,
                    }),
                    k8s: None,
                },
            },
        )
    }

    fn infra_agent_default_start_settings(
        hostname: &str,
        super_agent_instance_id: InstanceID,
        sub_agent_instance_id: InstanceID,
        agent_config: &SubAgentConfig,
    ) -> StartSettings {
        let identifying_attributes = HashMap::<String, DescriptionValueType>::from([
            (
                OPAMP_SERVICE_NAME.to_string(),
                agent_config.agent_type.name().into(),
            ),
            (
                OPAMP_SERVICE_NAMESPACE.to_string(),
                agent_config.agent_type.namespace().into(),
            ),
            (
                OPAMP_SERVICE_VERSION.to_string(),
                agent_config.agent_type.version().into(),
            ),
            (
                OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
                "0.0.0".into(),
            ),
        ]);
        StartSettings {
            instance_id: sub_agent_instance_id.into(),
            capabilities: default_capabilities(),
            agent_description: AgentDescription {
                identifying_attributes,
                non_identifying_attributes: HashMap::from([
                    (
                        HOST_NAME_ATTRIBUTE_KEY.to_string(),
                        DescriptionValueType::String(hostname.to_string()),
                    ),
                    (
                        PARENT_AGENT_ID_ATTRIBUTE_KEY.to_string(),
                        DescriptionValueType::Bytes(super_agent_instance_id.into()),
                    ),
                ]),
            },
        }
    }
}
