#[derive(Default)]
pub struct AgentControlCommonConfigBuilder {
    pub opamp_endpoint: Option<String>,
    pub jwks_endpoint: Option<String>,
    pub agents: Option<String>,
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

    pub fn build_agents_yaml(&self) -> String {
        let agents = self.agents.as_deref().unwrap_or("{}");
        format!("agents: {agents}")
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
