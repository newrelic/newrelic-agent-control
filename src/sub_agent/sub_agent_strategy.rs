use crate::opamp::client_builder::OpAMPClientBuilder;
use crate::sub_agent::k8s::sub_agent_k8s::{NotStartedSubAgentK8S, StartedSubAgentK8S};
use crate::sub_agent::on_host::sub_agent_on_host::{
    NotStartedSubAgentOnHost, StartedSubAgentOnHost,
};
use crate::sub_agent::sub_agent::{NotStartedSubAgent, StartedSubAgent, SubAgentError};
use crate::super_agent::instance_id::InstanceIDGetter;
use opamp_client::StartedClient;
use std::thread::JoinHandle;

pub enum NotStartedSubAgentStrategy<'a, OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    OnHost(NotStartedSubAgentOnHost<'a, OpAMPBuilder, ID>),
    K8S(NotStartedSubAgentK8S<'a, OpAMPBuilder, ID>),
}

impl<'a, OpAMPBuilder, ID> NotStartedSubAgentStrategy<'a, OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    pub fn run(self) -> Result<StartedSubAgentStrategy<OpAMPBuilder::Client>, SubAgentError> {
        match self {
            NotStartedSubAgentStrategy::OnHost(sub_agent) => {
                Ok(StartedSubAgentStrategy::OnHost(sub_agent.run()?))
            }
            NotStartedSubAgentStrategy::K8S(sub_agent) => {
                Ok(StartedSubAgentStrategy::K8S(sub_agent.run()?))
            }
        }
    }
}

pub enum StartedSubAgentStrategy<C>
where
    C: StartedClient,
{
    OnHost(StartedSubAgentOnHost<C>),
    K8S(StartedSubAgentK8S<C>),
}

impl<C> StartedSubAgentStrategy<C>
where
    C: StartedClient,
{
    pub fn stop(self) -> Result<Vec<JoinHandle<()>>, SubAgentError> {
        match self {
            StartedSubAgentStrategy::OnHost(sub_agent) => Ok(sub_agent.stop()?),
            StartedSubAgentStrategy::K8S(sub_agent) => Ok(sub_agent.stop()?),
        }
    }
}
