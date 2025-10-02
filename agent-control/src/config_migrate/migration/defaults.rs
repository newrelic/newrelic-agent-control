// We are not disallowing by code to put two config entries with the same agent_type_fqn,
// but there should be only one entry for each because the last one will overwrite the previous ones.
pub const NEWRELIC_INFRA_AGENT_TYPE_CONFIG_MAPPING: &str = r#"
configs:
  -
    agent_type_fqn: newrelic/com.newrelic.infrastructure:0.1.0
    files_map:
      config_agent: /etc/newrelic-infra.yml
    dirs_map:
      config_integrations:
        path: /etc/newrelic-infra/integrations.d
        extensions:
          - "yml"
          - "yaml"
      config_logging:
        path: /etc/newrelic-infra/logging.d
        extensions:
          - "yml"
          - "yaml"
"#;
