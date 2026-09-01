use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

#[derive(Default)]
pub struct AgentControlCommonConfigBuilder {
    pub opamp_endpoint: Option<String>,
    pub jwks_endpoint: Option<String>,
    pub agents: Vec<(String, String)>,
    pub agents_raw: Option<String>,
    pub status_server_port: Option<u16>,
    pub signature_validation_disabled: bool,
}

impl AgentControlCommonConfigBuilder {
    pub fn with_fleet(
        mut self,
        opamp_endpoint: impl Into<String>,
        jwks_endpoint: impl Into<String>,
    ) -> Self {
        self.opamp_endpoint = Some(opamp_endpoint.into());
        self.jwks_endpoint = Some(jwks_endpoint.into());
        self
    }

    pub fn build_fleet_control_yaml(&self) -> String {
        let (Some(endpoint), Some(jwks)) = (&self.opamp_endpoint, &self.jwks_endpoint) else {
            return String::new();
        };

        if !self.signature_validation_disabled {
            format!(
                r#"fleet_control:
  endpoint: {endpoint}
  poll_interval: 5s
  signature_validation:
    public_key_server_url: {jwks}"#
            )
        } else {
            format!(
                r#"fleet_control:
  endpoint: {endpoint}
  poll_interval: 5s
  signature_validation:
    enabled: false"#
            )
        }
    }

    pub fn with_agent(
        mut self,
        agent_id: impl Into<String>,
        agent_type: impl Into<String>,
    ) -> Self {
        self.agents.push((agent_id.into(), agent_type.into()));
        self
    }

    pub fn with_agents(mut self, agents: impl Into<String>) -> Self {
        self.agents_raw = Some(agents.into());
        self
    }

    pub fn build_agents_yaml(&self) -> String {
        if let Some(raw) = &self.agents_raw {
            let agents: serde_json::Value = serde_saphyr::from_str(raw).unwrap();
            let agents_config = HashMap::from([("agents".to_string(), agents)]);
            return serde_saphyr::to_string(&agents_config).unwrap();
        }

        #[derive(Serialize)]
        struct AgentEntry {
            agent_type: String,
        }

        let agents: BTreeMap<String, AgentEntry> = self
            .agents
            .iter()
            .map(|(id, agent_type)| {
                (
                    id.clone(),
                    AgentEntry {
                        agent_type: agent_type.clone(),
                    },
                )
            })
            .collect();
        let agents_config = HashMap::from([("agents".to_string(), agents)]);
        serde_saphyr::to_string(&agents_config).unwrap()
    }

    pub fn build_server_yaml(&self) -> String {
        self.status_server_port
            .map(|port| {
                format!(
                    r#"server:
  enabled: true
  port: {port}"#
                )
            })
            .unwrap_or_default()
    }
}
