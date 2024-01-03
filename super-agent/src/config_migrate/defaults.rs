// Infrastructure_agent AgentType
pub const DEFAULT_AGENT_ID: &str = "nr_infra_agent";

pub const NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING: &str = r#"
configs:
  -
    agent_type_fqn: newrelic/com.newrelic.infrastructure_agent:0.0.2
    files_map:
      config_agent: /etc/newrelic-infra.yml
    dirs_map:
      config_ohis: /etc/newrelic-infra/integrations.d
      logging: /etc/newrelic-infra/logging.d
"#;
