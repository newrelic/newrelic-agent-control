use crate::{
    config::{agent_type::agent_types::FinalAgent, super_agent_configs::AgentID},
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::{
        error::SubAgentBuilderError, on_host::sub_agent::NotStartedSubAgentOnHost, SubAgentBuilder,
    },
    super_agent::instance_id::InstanceIDGetter,
};

pub struct K8sSubAgentBuilder<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    _opamp_builder: Option<&'a O>,
    _instance_id_getter: &'a I,
}

impl<'a, O, I> K8sSubAgentBuilder<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    pub fn new(opamp_builder: Option<&'a O>, instance_id_getter: &'a I) -> Self {
        Self {
            _opamp_builder: opamp_builder,
            _instance_id_getter: instance_id_getter,
        }
    }
}

impl<'a, O, I> SubAgentBuilder for K8sSubAgentBuilder<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    type NotStartedSubAgent = NotStartedSubAgentOnHost<'a, O, I>;
    fn build(
        &self,
        _agent: FinalAgent,
        _agent_id: AgentID,
        _tx: std::sync::mpsc::Sender<crate::sub_agent::on_host::command::stream::Event>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        unimplemented!()
    }
}
