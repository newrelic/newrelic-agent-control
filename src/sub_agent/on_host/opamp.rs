use crate::config::super_agent_configs::{AgentID, AgentTypeFQN};
use crate::context::Context;
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError};
use crate::super_agent::instance_id::InstanceIDGetter;
use crate::super_agent::super_agent::SuperAgentEvent;
use nix::unistd::gethostname;
use opamp_client::capabilities;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::settings::{AgentDescription, StartSettings};
use std::collections::HashMap;

pub(super) fn build_opamp_and_start_client<OpAMPBuilder, InstanceIdGetter>(
    ctx: Context<Option<SuperAgentEvent>>,
    opamp_builder: Option<&OpAMPBuilder>,
    instance_id_getter: &InstanceIdGetter,
    agent_id: AgentID,
    agent_type: &AgentTypeFQN,
) -> Result<Option<OpAMPBuilder::Client>, OpAMPClientBuilderError>
where
    OpAMPBuilder: OpAMPClientBuilder,
    InstanceIdGetter: InstanceIDGetter,
{
    match opamp_builder {
        Some(builder) => {
            let start_settings = start_settings(instance_id_getter.get(&agent_id), agent_type);

            Ok(Some(builder.build_and_start(
                ctx,
                agent_id,
                start_settings,
            )?))
        }
        None => Ok(None),
    }
}

fn start_settings(instance_id: String, agent_fqn: &AgentTypeFQN) -> StartSettings {
    StartSettings {
        instance_id,
        capabilities: capabilities!(AgentCapabilities::ReportsHealth),
        agent_description: AgentDescription {
            identifying_attributes: HashMap::from([
                ("service.name".to_string(), agent_fqn.name().into()),
                (
                    "service.namespace".to_string(),
                    agent_fqn.namespace().into(),
                ),
                ("service.version".to_string(), agent_fqn.version().into()),
            ]),
            non_identifying_attributes: HashMap::from([(
                "host.name".to_string(),
                get_hostname().into(),
            )]),
        },
    }
}

fn get_hostname() -> String {
    #[cfg(unix)]
    return gethostname().unwrap_or_default().into_string().unwrap();

    #[cfg(not(unix))]
    return unimplemented!();
}
