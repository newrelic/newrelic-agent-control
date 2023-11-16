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
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        tx: std::sync::mpsc::Sender<Event>,
        ctx: Context<Option<SuperAgentEvent>>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        unimplemented!()
    }
}

use crate::config::super_agent_configs::{AgentTypeFQN, SubAgentConfig};
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
use crate::super_agent::effective_agents_assembler::EffectiveAgentsAssemblerError;
use crate::super_agent::super_agent::SuperAgentEvent;
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
