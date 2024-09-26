use super::{
    effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssemblerError},
    error::SubAgentBuilderError,
};

pub trait SupervisorBuilder {
    type Supervisor;
    type OpAMPClient;

    fn build_supervisor(
        &self,
        effective_agent_result: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
        maybe_opamp_client: &Option<Self::OpAMPClient>,
    ) -> Result<Option<Self::Supervisor>, SubAgentBuilderError>;
}
