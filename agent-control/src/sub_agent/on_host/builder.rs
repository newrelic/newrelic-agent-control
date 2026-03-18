use crate::agent_control::defaults::{
    HOST_NAME_ATTRIBUTE_KEY, OPAMP_SERVICE_VERSION, OS_ATTRIBUTE_KEY, OS_ATTRIBUTE_VALUE,
};
use crate::agent_control::run::Environment;
use crate::event::SubAgentEvent;
use crate::event::broadcaster::unbounded::UnboundedBroadcast;
use crate::event::channel::pub_sub;
use crate::opamp::client_builder::BuildOpAMPClient;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::build_sub_agent_opamp;
use crate::package::manager::PackageManager;
use crate::sub_agent::SubAgent;
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::on_host::command::executable_data::ExecutableData;
use crate::sub_agent::on_host::supervisor::{NotStartedSupervisorOnHost, SupervisorError};
use crate::sub_agent::remote_config_parser::RemoteConfigParser;
use crate::sub_agent::supervisor::SupervisorBuilder;
use crate::sub_agent::{SubAgentBuilder, error::SubAgentBuilderError};
use crate::values::config_repository::ConfigRepository;
use opamp_client::operation::settings::DescriptionValueType;
use resource_detection::system::hostname::get_hostname;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, instrument};

pub struct OnHostSubAgentBuilder<'a, O, I, B, R, Y, A>
where
    O: BuildOpAMPClient,
    I: InstanceIDGetter,
    B: SupervisorBuilder + Send + Sync + 'static,
    R: RemoteConfigParser + Send + Sync + 'static,
    Y: ConfigRepository + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
{
    pub(crate) opamp_builder: Option<&'a O>,
    pub(crate) instance_id_getter: &'a I,
    pub(crate) supervisor_builder: Arc<B>,
    pub(crate) remote_config_parser: Arc<R>,
    pub(crate) yaml_config_repository: Arc<Y>,
    pub(crate) effective_agents_assembler: Arc<A>,
    pub(crate) sub_agent_publisher: UnboundedBroadcast<SubAgentEvent>,
    pub(crate) ac_running_mode: Environment,
}

impl<O, I, B, R, Y, A> SubAgentBuilder for OnHostSubAgentBuilder<'_, O, I, B, R, Y, A>
where
    O: BuildOpAMPClient + Send + Sync + 'static,
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
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        debug!("building subAgent");

        let hostname = get_hostname()
            .map_err(|e| SubAgentBuilderError::OpampClientBuilderError(e.to_string()))?
            .into();

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
                    HashMap::from([
                        (HOST_NAME_ATTRIBUTE_KEY.to_string(), hostname),
                        (
                            OS_ATTRIBUTE_KEY.to_string(),
                            DescriptionValueType::String(OS_ATTRIBUTE_VALUE.to_string()),
                        ),
                    ]),
                )
                .map_err(|e| SubAgentBuilderError::OpampClientBuilderError(e.to_string()))
            })
            // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
            .transpose()?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();

        Ok(SubAgent::new(
            agent_identity.clone(),
            maybe_opamp_client,
            self.supervisor_builder.clone(),
            self.sub_agent_publisher.clone(),
            sub_agent_opamp_consumer,
            pub_sub(),
            self.remote_config_parser.clone(),
            self.yaml_config_repository.clone(),
            self.effective_agents_assembler.clone(),
            self.ac_running_mode,
        ))
    }
}

pub struct SupervisorBuilderOnHost<PM>
where
    PM: PackageManager,
{
    pub logging_path: PathBuf,
    pub package_manager: Arc<PM>,
}

impl<PM> SupervisorBuilder for SupervisorBuilderOnHost<PM>
where
    PM: PackageManager,
{
    type Starter = NotStartedSupervisorOnHost<PM>;
    type Error = SupervisorError;

    fn build_supervisor(
        &self,
        effective_agent: EffectiveAgent,
    ) -> Result<Self::Starter, Self::Error> {
        debug!(
            "Building Executable supervisors {}",
            effective_agent.get_agent_identity(),
        );
        let agent_identity = effective_agent.get_agent_identity().clone();

        let on_host = effective_agent
            .get_onhost_config()
            .map_err(SupervisorError::RuntimeConfig)?
            .clone();

        let executables = on_host
            .executables
            .into_iter()
            .map(|e| {
                ExecutableData::new(e.id, e.path)
                    .with_args(e.args.0)
                    .with_env(e.env.0)
                    .with_restart_policy(e.restart_policy.into())
            })
            .collect();

        Ok(NotStartedSupervisorOnHost::new(
            agent_identity,
            executables,
            on_host.health,
            on_host.version,
            on_host.packages,
            self.package_manager.clone(),
            on_host.enable_file_logging,
            self.logging_path.to_path_buf(),
            on_host.filesystem,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::opamp::client_builder::tests::MockOpAMPClientBuilder;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClient;
    use crate::opamp::instance_id::InstanceID;
    use crate::opamp::instance_id::getter::tests::MockInstanceIDGetter;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssembler;
    use crate::sub_agent::remote_config_parser::tests::MockRemoteConfigParser;
    use crate::sub_agent::supervisor::tests::MockSupervisorStarter;
    use crate::sub_agent::supervisor::tests::{MockSupervisor, MockSupervisorBuilder};
    use crate::values::config_repository::tests::MockConfigRepository;
    use std::time::Duration;

    #[test]
    fn test_build_with_opamp() {
        let agent_control_instance_id = InstanceID::create();
        let sub_agent_instance_id = InstanceID::create();
        let agent_identity = AgentIdentity::from((
            AgentID::try_from("infra-agent").unwrap(),
            AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.2").unwrap(),
        ));

        // Build an OpAMP Client and let it run so the publisher is not dropped
        let mut opamp_builder = MockOpAMPClientBuilder::new();
        opamp_builder.should_build_and_start_and_run(
            MockStartedOpAMPClient::new(),
            Duration::from_millis(10),
        );

        let mut instance_id_getter = MockInstanceIDGetter::new();
        instance_id_getter.should_get(&agent_identity.id, sub_agent_instance_id.clone());
        instance_id_getter.should_get(&AgentID::AgentControl, agent_control_instance_id.clone());

        let supervisor_builder =
            MockSupervisorBuilder::<MockSupervisorStarter<MockSupervisor>>::new();

        let on_host_builder = OnHostSubAgentBuilder {
            opamp_builder: Some(&opamp_builder),
            instance_id_getter: &instance_id_getter,
            supervisor_builder: Arc::new(supervisor_builder),
            remote_config_parser: Arc::new(MockRemoteConfigParser::new()),
            yaml_config_repository: Arc::new(MockConfigRepository::new()),
            effective_agents_assembler: Arc::new(MockEffectiveAgentAssembler::new()),
            sub_agent_publisher: UnboundedBroadcast::default(),
            ac_running_mode: AGENT_CONTROL_MODE_ON_HOST,
        };

        assert!(on_host_builder.build(&agent_identity).is_ok());
    }
}
