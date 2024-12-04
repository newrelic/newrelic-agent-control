use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::version::version_checker::AgentVersion;
use crate::super_agent::defaults::OPAMP_AGENT_VERSION_ATTRIBUTE_KEY;
use opamp_client::opamp::proto::{any_value, AgentDescription, AnyValue, KeyValue};
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;

pub fn on_version<C, CB>(
    version: AgentVersion,
    maybe_opamp_client: Option<&C>,
) -> Result<(), SubAgentError>
where
    C: StartedClient<CB>,
    CB: Callbacks,
{
    if let Some(client) = maybe_opamp_client.as_ref() {
        let agent_description = client.get_agent_description();
        client.set_agent_description(add_version_to_agent_description(
            agent_description?,
            version.version().to_string(),
        ))?;
    }
    Ok(())
}

fn add_version_to_agent_description(
    mut agent_description: AgentDescription,
    version: String,
) -> AgentDescription {
    agent_description.identifying_attributes.push(KeyValue {
        key: OPAMP_AGENT_VERSION_ATTRIBUTE_KEY.to_string(),
        value: Some(AnyValue {
            value: Option::from(any_value::Value::StringValue(version)),
        }),
    });
    agent_description
}
