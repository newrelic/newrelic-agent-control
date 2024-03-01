use crate::{
    sub_agent::persister::config_persister::ConfigurationPersister, super_agent::config::AgentID,
};

use super::{
    agent_values::AgentValues,
    definition::{AgentAttributes, AgentType},
    error::AgentTypeError,
    runtime_config::Runtime,
};

/// Defines how to render an AgentType and obtain the runtime configuration needed to execute a sub agent.
pub trait Renderer {
    /// Renders the runtime configuration in an [AgentType] using the provided values and attributes.
    fn render(
        &self,
        agent_id: &AgentID,
        agent_type: AgentType,
        values: AgentValues,
        attributes: AgentAttributes,
    ) -> Result<Runtime, AgentTypeError>;
}

pub struct TemplateRenderer<C: ConfigurationPersister> {
    persister: Option<C>, // TODO: check if it should be optional or we should have different Renderer implementations.
                          // depending on what fields are supported for each environment.
}

impl<C: ConfigurationPersister> Renderer for TemplateRenderer<C> {
    fn render(
        &self,
        agent_id: &AgentID,
        agent_type: AgentType,
        values: AgentValues,
        attributes: AgentAttributes,
    ) -> Result<Runtime, AgentTypeError> {
        // TODO: `agent_type.template` logic (and underlying helper methods) should be moved here.
        let populated_agent = agent_type.template(values, attributes)?;
        if let Some(persister) = &self.persister {
            persister.delete_agent_config(agent_id, &populated_agent)?;
            persister.persist_agent_config(agent_id, &populated_agent)?;
        }
        Ok(populated_agent.runtime_config)
    }
}

impl<C: ConfigurationPersister> Default for TemplateRenderer<C> {
    fn default() -> Self {
        Self { persister: None }
    }
}

impl<C: ConfigurationPersister> TemplateRenderer<C> {
    pub fn with_config_persister(self, c: C) -> Self {
        Self { persister: Some(c) }
    }
}
