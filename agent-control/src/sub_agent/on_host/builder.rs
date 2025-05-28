use crate::agent_control::defaults::{HOST_NAME_ATTRIBUTE_KEY, OPAMP_SERVICE_VERSION};
use crate::agent_control::run::Environment;
use crate::context::Context;
use crate::event::SubAgentEvent;
use crate::event::broadcaster::unbounded::UnboundedBroadcast;
use crate::event::channel::pub_sub;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::build_sub_agent_opamp;
use crate::sub_agent::SubAgent;
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use crate::sub_agent::on_host::supervisor::NotStartedSupervisorOnHost;
use crate::sub_agent::remote_config_parser::RemoteConfigParser;
use crate::sub_agent::supervisor::builder::SupervisorBuilder;
use crate::values::config_repository::ConfigRepository;
use crate::{
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::{SubAgentBuilder, error::SubAgentBuilderError},
};
#[cfg(unix)]
use nix::unistd::gethostname;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, instrument};

pub struct OnHostSubAgentBuilder<'a, O, I, B, R, Y, A>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
    B: SupervisorBuilder + Send + Sync + 'static,
    R: RemoteConfigParser + Send + Sync + 'static,
    Y: ConfigRepository + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,
    supervisor_builder: Arc<B>,
    remote_config_parser: Arc<R>,
    yaml_config_repository: Arc<Y>,
    effective_agents_assembler: Arc<A>,
}

impl<'a, O, I, B, R, Y, A> OnHostSubAgentBuilder<'a, O, I, B, R, Y, A>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
    B: SupervisorBuilder + Send + Sync + 'static,
    R: RemoteConfigParser + Send + Sync + 'static,
    Y: ConfigRepository + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
{
    pub fn new(
        opamp_builder: Option<&'a O>,
        instance_id_getter: &'a I,
        supervisor_builder: Arc<B>,
        remote_config_parser: Arc<R>,
        yaml_config_repository: Arc<Y>,
        effective_agents_assembler: Arc<A>,
    ) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,
            supervisor_builder,
            remote_config_parser,
            yaml_config_repository,
            effective_agents_assembler,
        }
    }
}

impl<O, I, B, R, Y, A> SubAgentBuilder for OnHostSubAgentBuilder<'_, O, I, B, R, Y, A>
where
    O: OpAMPClientBuilder + Send + Sync + 'static,
    I: InstanceIDGetter,
    B: SupervisorBuilder + Send + Sync + 'static,
    R: RemoteConfigParser + Send + Sync + 'static,
    Y: ConfigRepository + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
{
    type NotStartedSubAgent = SubAgent<O::Client, B, R, Y, A>;

    #[instrument(skip_all, fields(id = %agent_identity.id),name = "build_agent")]
    fn build(
        &self,
        agent_identity: &AgentIdentity,
        sub_agent_publisher: UnboundedBroadcast<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        debug!("building subAgent");

        let (maybe_opamp_client, sub_agent_opamp_consumer) = self
            .opamp_builder
            .map(|builder| {
                build_sub_agent_opamp(
                    builder,
                    self.instance_id_getter,
                    agent_identity,
                    HashMap::from([(
                        OPAMP_SERVICE_VERSION.to_string(),
                        agent_identity.agent_type_id.version().to_string().into(),
                    )]),
                    HashMap::from([(HOST_NAME_ATTRIBUTE_KEY.to_string(), get_hostname().into())]),
                )
            })
            // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
            .transpose()?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();

        Ok(SubAgent::new(
            agent_identity.clone(),
            maybe_opamp_client,
            self.supervisor_builder.clone(),
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            pub_sub(),
            self.remote_config_parser.clone(),
            self.yaml_config_repository.clone(),
            self.effective_agents_assembler.clone(),
            Environment::OnHost,
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
        debug!(
            "Building Executable supervisors {}",
            effective_agent.get_agent_identity(),
        );

        let on_host = effective_agent.get_onhost_config()?.clone();

        let enable_file_logging = on_host.enable_file_logging.get();

        let maybe_exec = on_host.executable.map(|e| {
            ExecutableData::new(e.path.get())
                .with_args(e.args.get().into_vector())
                .with_env(e.env.get())
                .with_restart_policy(e.restart_policy.into())
        });

        let executable_supervisors = NotStartedSupervisorOnHost::new(
            effective_agent.get_agent_identity().clone(),
            maybe_exec,
            Context::new(),
            on_host.health,
        )
        .with_file_logging(enable_file_logging, self.logging_path.to_path_buf());

        Ok(executable_supervisors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;

    use crate::agent_control::defaults::{
        OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE, OPAMP_SERVICE_VERSION,
        PARENT_AGENT_ID_ATTRIBUTE_KEY, default_capabilities, default_sub_agent_custom_capabilities,
    };
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::runtime_config::{Deployment, Runtime};
    use crate::opamp::client_builder::tests::MockOpAMPClientBuilder;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClient;
    use crate::opamp::instance_id::InstanceID;
    use crate::opamp::instance_id::getter::tests::MockInstanceIDGetter;
    use crate::opamp::remote_config::hash::Hash;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssembler;
    use crate::sub_agent::remote_config_parser::tests::MockRemoteConfigParser;
    use crate::sub_agent::supervisor::builder::tests::MockSupervisorBuilder;
    use crate::sub_agent::supervisor::starter::tests::MockSupervisorStarter;
    use crate::sub_agent::supervisor::stopper::tests::MockSupervisorStopper;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::values::config::{Config, RemoteConfig};
    use crate::values::config_repository::tests::MockConfigRepository;
    use crate::values::yaml_config::YAMLConfig;
    use mockall::predicate;
    use nix::unistd::gethostname;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::operation::settings::{
        AgentDescription, DescriptionValueType, StartSettings,
    };
    use std::collections::HashMap;
    use std::time::Duration;
    use tracing_test::traced_test;

    // TODO: tests below are testing not only the builder but also the sub-agent start/stop behavior.
    // We should re-consider their scope.
    #[test]
    fn build_start_stop() {
        let mut opamp_builder = MockOpAMPClientBuilder::new();
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let agent_identity = AgentIdentity::from((
            AgentID::new("infra-agent").unwrap(),
            AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.2").unwrap(),
        ));

        let remote_config_values =
            RemoteConfig::new(YAMLConfig::default(), Hash::new("a-hash".to_string()));

        let sub_agent_instance_id = InstanceID::create();
        let agent_control_instance_id = InstanceID::create();

        let start_settings_infra = infra_agent_default_start_settings(
            &hostname,
            agent_control_instance_id.clone(),
            sub_agent_instance_id.clone(),
            &agent_identity.agent_type_id,
        );

        let agent_control_id = AgentID::new_agent_control_id();

        let mut started_client = MockStartedOpAMPClient::new();
        // Report config status as applied
        let status = RemoteConfigStatus {
            status: opamp_client::opamp::proto::RemoteConfigStatuses::Applied as i32,
            last_remote_config_hash: Hash::new("a-hash".to_string()).get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);
        started_client.should_update_effective_config(1);
        started_client.should_stop(1);

        // Infra Agent OpAMP no final stop nor health, just after stopping on reload
        // TODO: We should discuss if this is a valid approach once we refactor the unit tests
        // Build an OpAMP Client and let it run so the publisher is not dropped
        opamp_builder.should_build_and_start_and_run(
            agent_identity.id.clone(),
            start_settings_infra,
            started_client,
            Duration::from_millis(10),
        );

        let mut config_repository = MockConfigRepository::new();
        config_repository
            .expect_load_remote()
            .with(
                predicate::eq(agent_identity.id.clone()),
                predicate::always(),
            )
            .once()
            .return_once(move |_, _| Ok(Some(Config::RemoteConfig(remote_config_values))));

        let mut hash = Hash::new("a-hash".to_string());
        hash.apply();
        config_repository
            .expect_update_hash_state()
            .with(
                predicate::eq(agent_identity.id.clone()),
                predicate::eq(hash.state()),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        let mut instance_id_getter = MockInstanceIDGetter::new();
        instance_id_getter.should_get(&agent_identity.id, sub_agent_instance_id.clone());
        instance_id_getter.should_get(&agent_control_id, agent_control_instance_id.clone());

        let mut started_supervisor = MockSupervisorStopper::new();
        started_supervisor.should_stop();

        let mut stopped_supervisor = MockSupervisorStarter::new();
        stopped_supervisor.should_start(started_supervisor);

        let mut effective_agents_assembler = MockEffectiveAgentAssembler::new();
        let effective_agent = EffectiveAgent::new(
            agent_identity.clone(),
            Runtime {
                deployment: Deployment::default(),
            },
        );
        effective_agents_assembler.should_assemble_agent(
            &agent_identity,
            &YAMLConfig::default(),
            &Environment::OnHost,
            effective_agent.clone(),
            1,
        );

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::eq(effective_agent))
            .return_once(|_| Ok(stopped_supervisor));

        let remote_config_parser = MockRemoteConfigParser::new();

        let on_host_builder = OnHostSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(supervisor_builder),
            Arc::new(remote_config_parser),
            Arc::new(config_repository),
            Arc::new(effective_agents_assembler),
        );

        on_host_builder
            .build(&agent_identity, UnboundedBroadcast::default())
            .unwrap()
            .run()
            .stop()
            .unwrap();
    }

    //TODO This test doesn't make any sense here (probably it doesn't make sense to exist at all)
    #[traced_test]
    #[test]
    fn test_subagent_should_report_failed_config() {
        // Mocks
        let mut opamp_builder = MockOpAMPClientBuilder::new();
        let mut instance_id_getter = MockInstanceIDGetter::new();

        // Structures
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let agent_identity = AgentIdentity::from((
            AgentID::new("infra-agent").unwrap(),
            AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.2").unwrap(),
        ));
        let sub_agent_instance_id = InstanceID::create();
        let agent_control_instance_id = InstanceID::create();

        let start_settings_infra = infra_agent_default_start_settings(
            &hostname,
            agent_control_instance_id.clone(),
            sub_agent_instance_id.clone(),
            &agent_identity.agent_type_id,
        );

        let remote_config_values =
            RemoteConfig::new(YAMLConfig::default(), Hash::new("a-hash".to_string()));

        let agent_control_id = AgentID::new_agent_control_id();
        // Expectations
        // Infra Agent OpAMP no final stop nor health, just after stopping on reload
        instance_id_getter.should_get(&agent_identity.id, sub_agent_instance_id.clone());
        instance_id_getter.should_get(&agent_control_id, agent_control_instance_id.clone());

        let mut started_client = MockStartedOpAMPClient::new();
        started_client.should_update_effective_config(1);

        // Report config status as applied
        let status = RemoteConfigStatus {
            status: opamp_client::opamp::proto::RemoteConfigStatuses::Applied as i32,
            last_remote_config_hash: remote_config_values.config_hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);
        started_client.should_stop(1);

        // TODO: We should discuss if this is a valid approach once we refactor the unit tests
        // Build an OpAMP Client and let it run so the publisher is not dropped
        opamp_builder.should_build_and_start_and_run(
            agent_identity.id.clone(),
            start_settings_infra,
            started_client,
            Duration::from_millis(10),
        );

        let mut config_repository = MockConfigRepository::new();
        config_repository
            .expect_load_remote()
            .with(
                predicate::eq(agent_identity.id.clone()),
                predicate::always(),
            )
            .once()
            .return_once(move |_, _| Ok(Some(Config::RemoteConfig(remote_config_values.clone()))));

        let mut hash = Hash::new("a-hash".to_string());
        hash.apply();
        config_repository
            .expect_update_hash_state()
            .with(
                predicate::eq(agent_identity.id.clone()),
                predicate::eq(hash.state()),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        let mut started_supervisor = MockSupervisorStopper::new();
        started_supervisor.should_stop();

        let mut stopped_supervisor = MockSupervisorStarter::new();
        stopped_supervisor.should_start(started_supervisor);

        let mut effective_agents_assembler = MockEffectiveAgentAssembler::new();
        let effective_agent = EffectiveAgent::new(
            agent_identity.clone(),
            Runtime {
                deployment: Deployment::default(),
            },
        );
        effective_agents_assembler.should_assemble_agent(
            &agent_identity,
            &YAMLConfig::default(),
            &Environment::OnHost,
            effective_agent.clone(),
            1,
        );

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::eq(effective_agent))
            .return_once(|_| Ok(stopped_supervisor));

        let remote_config_parser = MockRemoteConfigParser::new();

        // Sub Agent Builder
        let on_host_builder = OnHostSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(supervisor_builder),
            Arc::new(remote_config_parser),
            Arc::new(config_repository),
            Arc::new(effective_agents_assembler),
        );

        let sub_agent = on_host_builder
            .build(&agent_identity, UnboundedBroadcast::default())
            .expect("Subagent build should be OK");
        let started_sub_agent = sub_agent.run(); // Running the sub-agent should report the failed configuration.
        started_sub_agent.stop().unwrap();
    }

    // HELPERS
    fn infra_agent_default_start_settings(
        hostname: &str,
        agent_control_instance_id: InstanceID,
        sub_agent_instance_id: InstanceID,
        agent_fqn: &AgentTypeID,
    ) -> StartSettings {
        let identifying_attributes = HashMap::<String, DescriptionValueType>::from([
            (OPAMP_SERVICE_NAME.to_string(), agent_fqn.name().into()),
            (
                OPAMP_SERVICE_NAMESPACE.to_string(),
                agent_fqn.namespace().into(),
            ),
            (
                OPAMP_SERVICE_VERSION.to_string(),
                agent_fqn.version().to_string().into(),
            ),
        ]);
        StartSettings {
            instance_uid: sub_agent_instance_id.into(),
            capabilities: default_capabilities(),
            custom_capabilities: Some(default_sub_agent_custom_capabilities()),
            agent_description: AgentDescription {
                identifying_attributes,
                non_identifying_attributes: HashMap::from([
                    (
                        HOST_NAME_ATTRIBUTE_KEY.to_string(),
                        DescriptionValueType::String(hostname.to_string()),
                    ),
                    (
                        PARENT_AGENT_ID_ATTRIBUTE_KEY.to_string(),
                        DescriptionValueType::Bytes(agent_control_instance_id.into()),
                    ),
                ]),
            },
        }
    }
}
