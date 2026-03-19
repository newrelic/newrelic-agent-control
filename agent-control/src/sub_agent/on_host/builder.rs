use crate::agent_control::defaults::{
    HOST_NAME_ATTRIBUTE_KEY, OS_ATTRIBUTE_KEY, OS_ATTRIBUTE_VALUE,
};
use crate::agent_control::run::Environment;
use crate::event::SubAgentEvent;
use crate::event::broadcaster::unbounded::UnboundedBroadcast;
use crate::event::channel::pub_sub;
use crate::opamp::client_builder::BuildOpAMPClient;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::{
    agent_control_service_version_attribute, maybe_build_sub_agent_opamp,
};
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

pub struct OnHostSubAgentBuilder<O, I, B, R, Y, A>
where
    O: BuildOpAMPClient,
    I: InstanceIDGetter,
    B: SupervisorBuilder + Send + Sync + 'static,
    R: RemoteConfigParser + Send + Sync + 'static,
    Y: ConfigRepository + Send + Sync + 'static,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
{
    pub(crate) opamp_builder: Option<O>,
    pub(crate) instance_id_getter: I,
    pub(crate) supervisor_builder: Arc<B>,
    pub(crate) remote_config_parser: Arc<R>,
    pub(crate) yaml_config_repository: Arc<Y>,
    pub(crate) effective_agents_assembler: Arc<A>,
    pub(crate) sub_agent_publisher: UnboundedBroadcast<SubAgentEvent>,
    pub(crate) ac_running_mode: Environment,
}

impl<O, I, B, R, Y, A> SubAgentBuilder for OnHostSubAgentBuilder<O, I, B, R, Y, A>
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

        let (maybe_opamp_client, sub_agent_opamp_consumer) = maybe_build_sub_agent_opamp(
            self.opamp_builder.as_ref(),
            &self.instance_id_getter,
            agent_identity,
            agent_control_service_version_attribute(agent_identity.agent_type_id.version()),
            get_onhost_extra_non_identifying_attributes()?,
        )
        .map_err(|e| SubAgentBuilderError::OpampClientBuilderError(e.to_string()))?
        .unzip();

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

fn get_onhost_extra_non_identifying_attributes()
-> Result<HashMap<String, DescriptionValueType>, SubAgentBuilderError> {
    let hostname = get_hostname()
        .map_err(|e| SubAgentBuilderError::OpampClientBuilderError(e.to_string()))?
        .into();

    Ok(HashMap::from([
        (HOST_NAME_ATTRIBUTE_KEY.to_string(), hostname),
        (
            OS_ATTRIBUTE_KEY.to_string(),
            DescriptionValueType::String(OS_ATTRIBUTE_VALUE.to_string()),
        ),
    ]))
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
    use crate::agent_control::defaults::{
        OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE, OPAMP_SERVICE_VERSION, OPAMP_SUPERVISOR_KEY,
        PARENT_AGENT_ID_ATTRIBUTE_KEY, default_capabilities, default_custom_capabilities,
    };
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
    use opamp_client::operation::settings::{
        AgentDescription, DescriptionValueType, StartSettings,
    };
    use std::collections::HashMap;
    use std::time::Duration;

    #[test]
    fn test_build_with_opamp() {
        let hostname = get_hostname().unwrap();
        let agent_control_instance_id = InstanceID::create();
        let sub_agent_instance_id = InstanceID::create();
        let agent_identity = AgentIdentity::from((
            AgentID::try_from("infra-agent").unwrap(),
            AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.2").unwrap(),
        ));

        let start_settings_infra = infra_agent_default_start_settings(
            &hostname,
            agent_control_instance_id.clone(),
            sub_agent_instance_id.clone(),
            &agent_identity,
        );

        // Build an OpAMP Client and let it run so the publisher is not dropped
        let mut opamp_builder = MockOpAMPClientBuilder::new();
        opamp_builder.should_build_and_start_and_run(
            agent_identity.clone(),
            start_settings_infra,
            MockStartedOpAMPClient::new(),
            Duration::from_millis(10),
        );

        let mut instance_id_getter = MockInstanceIDGetter::new();
        instance_id_getter.should_get(&agent_identity.id, sub_agent_instance_id.clone());
        instance_id_getter.should_get(&AgentID::AgentControl, agent_control_instance_id.clone());

        let supervisor_builder =
            MockSupervisorBuilder::<MockSupervisorStarter<MockSupervisor>>::new();

        let on_host_builder = OnHostSubAgentBuilder {
            opamp_builder: Some(opamp_builder),
            instance_id_getter,
            supervisor_builder: Arc::new(supervisor_builder),
            remote_config_parser: Arc::new(MockRemoteConfigParser::new()),
            yaml_config_repository: Arc::new(MockConfigRepository::new()),
            effective_agents_assembler: Arc::new(MockEffectiveAgentAssembler::new()),
            sub_agent_publisher: UnboundedBroadcast::default(),
            ac_running_mode: AGENT_CONTROL_MODE_ON_HOST,
        };

        assert!(on_host_builder.build(&agent_identity).is_ok());
    }

    // HELPERS
    fn infra_agent_default_start_settings(
        hostname: &str,
        agent_control_instance_id: InstanceID,
        sub_agent_instance_id: InstanceID,
        agent_identity: &AgentIdentity,
    ) -> StartSettings {
        let identifying_attributes = HashMap::<String, DescriptionValueType>::from([
            (
                OPAMP_SERVICE_NAME.to_string(),
                agent_identity.agent_type_id.name().into(),
            ),
            (
                OPAMP_SERVICE_NAMESPACE.to_string(),
                agent_identity.agent_type_id.namespace().into(),
            ),
            (
                OPAMP_SUPERVISOR_KEY.to_string(),
                agent_identity.id.to_string().into(),
            ),
            (
                OPAMP_SERVICE_VERSION.to_string(),
                agent_identity.agent_type_id.version().to_string().into(),
            ),
        ]);
        StartSettings {
            instance_uid: sub_agent_instance_id.into(),
            capabilities: default_capabilities(),
            custom_capabilities: Some(default_custom_capabilities()),
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
                    (OS_ATTRIBUTE_KEY.to_string(), OS_ATTRIBUTE_VALUE.into()),
                ]),
            },
        }
    }
}
