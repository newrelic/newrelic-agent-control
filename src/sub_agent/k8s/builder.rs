use crate::{
    config::{agent_type::agent_types::FinalAgent, super_agent_configs::AgentID},
    context::Context,
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::{
        error::{SubAgentBuilderError, SubAgentError},
        logger::Event,
        restart_policy::RestartPolicy,
        SubAgentBuilder,
    },
    super_agent::instance_id::InstanceIDGetter,
};

#[derive(Default)]
pub struct K8sSubAgentBuilder;

impl SubAgentBuilder for K8sSubAgentBuilder {
    type NotStartedSubAgent = K8sSubAgent;

    fn build(
        &self,
        agent: Result<FinalAgent, EffectiveAgentsAssemblerError>,
        agent_id: AgentID,
        agent_type: &AgentTypeFQN,
        tx: std::sync::mpsc::Sender<Event>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        unimplemented!()
    }
}

use crate::config::remote_config_hash::HashRepository;
use crate::config::super_agent_configs::AgentTypeFQN;
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
use crate::super_agent::effective_agents_assembler::EffectiveAgentsAssemblerError;
use std::thread::JoinHandle;

pub struct K8sSubAgent;

impl NotStartedSubAgent for K8sSubAgent {
    type StartedSubAgent = K8sSubAgent;

    fn run(self) -> Result<Self::StartedSubAgent, SubAgentError> {
        unimplemented!()
    }
}

impl StartedSubAgent for K8sSubAgent {
    fn stop(self) -> Result<Vec<JoinHandle<()>>, SubAgentError> {
        unimplemented!()
    }
}
