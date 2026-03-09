use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::SubAgentsMap;
use crate::agent_type::agent_type_id::AgentTypeID;

/// Resolves parent AgentTypeID to actual running agent_id(s) from the configuration.
/// This allows integrations to find their parent agents even when multiple instances exist.
pub trait ParentAgentResolver: Send + Sync {
    /// Resolves a parent AgentTypeID to a list of agent_ids that match that type.
    /// Returns empty vec if no matching agents are found.
    fn resolve_parent_agent_ids(
        &self,
        parent_agent_type: &AgentTypeID,
        agents: &SubAgentsMap,
    ) -> Vec<AgentID>;
}

/// Implementation that resolves parent agents from the SubAgentsMap.
#[derive(Default)]
pub struct DefaultParentAgentResolver;

impl ParentAgentResolver for DefaultParentAgentResolver {
    fn resolve_parent_agent_ids(
        &self,
        parent_agent_type: &AgentTypeID,
        agents: &SubAgentsMap,
    ) -> Vec<AgentID> {
        agents
            .iter()
            .filter_map(|(agent_id, config)| {
                if &config.agent_type == parent_agent_type {
                    Some(agent_id.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::config::SubAgentConfig;
    use std::collections::HashMap;

    #[test]
    fn test_resolve_single_parent() {
        let parent_type = AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.1.0").unwrap();
        let child_type = AgentTypeID::try_from("newrelic/com.newrelic.integration_redis:0.1.0").unwrap();

        let mut agents = HashMap::new();
        agents.insert(
            AgentID::try_from("infra-agent").unwrap(),
            SubAgentConfig {
                agent_type: parent_type.clone(),
            },
        );
        agents.insert(
            AgentID::try_from("redis-integration").unwrap(),
            SubAgentConfig {
                agent_type: child_type,
            },
        );

        let resolver = DefaultParentAgentResolver;
        let parent_ids = resolver.resolve_parent_agent_ids(&parent_type, &agents);

        assert_eq!(parent_ids.len(), 1);
        assert_eq!(parent_ids[0].to_string(), "infra-agent");
    }

    #[test]
    fn test_resolve_multiple_parents() {
        let parent_type = AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.1.0").unwrap();

        let mut agents = HashMap::new();
        agents.insert(
            AgentID::try_from("infra-agent-1").unwrap(),
            SubAgentConfig {
                agent_type: parent_type.clone(),
            },
        );
        agents.insert(
            AgentID::try_from("infra-agent-2").unwrap(),
            SubAgentConfig {
                agent_type: parent_type.clone(),
            },
        );

        let resolver = DefaultParentAgentResolver;
        let parent_ids = resolver.resolve_parent_agent_ids(&parent_type, &agents);

        assert_eq!(parent_ids.len(), 2);
        assert!(parent_ids.iter().any(|id| id.to_string() == "infra-agent-1"));
        assert!(parent_ids.iter().any(|id| id.to_string() == "infra-agent-2"));
    }

    #[test]
    fn test_resolve_no_parent() {
        let parent_type = AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.1.0").unwrap();
        let different_type = AgentTypeID::try_from("newrelic/com.newrelic.opentelemetry.collector:0.1.0").unwrap();

        let mut agents = HashMap::new();
        agents.insert(
            AgentID::try_from("otel-collector").unwrap(),
            SubAgentConfig {
                agent_type: different_type,
            },
        );

        let resolver = DefaultParentAgentResolver;
        let parent_ids = resolver.resolve_parent_agent_ids(&parent_type, &agents);

        assert!(parent_ids.is_empty());
    }
}
