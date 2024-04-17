// We are not disallowing by code to put two config entries with the same agent_type_fqn,
// but there should be only one entry for each because the last one will overwrite the previous ones.
pub const NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING: &str = r#"
configs:
  -
    agent_type_fqn: newrelic/com.newrelic.infrastructure_agent:0.0.2
    files_map:
      config_agent: /etc/newrelic-infra.yml
    dirs_map:
      config_ohis: /etc/newrelic-infra/integrations.d
      logging: /etc/newrelic-infra/logging.d
  -
    agent_type_fqn: newrelic/com.newrelic.infrastructure_agent:0.1.0
    files_map:
      config_agent: /etc/newrelic-infra.yml
    dirs_map:
      config_integrations: /etc/newrelic-infra/integrations.d
      config_logging: /etc/newrelic-infra/logging.d
  -
    agent_type_fqn: newrelic/com.newrelic.infrastructure_agent:0.1.1
    files_map:
      config_agent: /etc/newrelic-infra.yml
    dirs_map:
      config_integrations: /etc/newrelic-infra/integrations.d
      config_logging: /etc/newrelic-infra/logging.d
  -
    agent_type_fqn: newrelic/com.newrelic.infrastructure_agent:0.1.2
    files_map:
      config_agent: /etc/newrelic-infra.yml
    dirs_map:
      config_integrations: /etc/newrelic-infra/integrations.d
      config_logging: /etc/newrelic-infra/logging.d
"#;
