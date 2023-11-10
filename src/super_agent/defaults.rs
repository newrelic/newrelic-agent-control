pub const SUPER_AGENT_ID: &str = "super-agent";
pub const SUPER_AGENT_TYPE: &str = "com.newrelic.super_agent";
pub const SUPER_AGENT_NAMESPACE: &str = "newrelic";
pub const SUPER_AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Paths
pub const SUPER_AGENT_DATA_DIR: &str = "/var/lib/newrelic-super-agent";

// Infrastructure_agent AgentType
pub(crate) const NEWRELIC_INFRA_TYPE: &str = r#"
namespace: newrelic
name: com.newrelic.infrastructure_agent
version: 0.0.1
variables:
  config_file:
    description: "Newrelic infra configuration path"
    type: string
    required: false
    default: /etc/newrelic-infra.yml
deployment:
  on_host:
    executables:
      - path: /usr/bin/newrelic-infra
        args: "--config=${config_file}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay_seconds: 5s
"#;

// NRDOT AgentType
pub(crate) const NRDOT_TYPE: &str = r#"
namespace: newrelic
name: io.opentelemetry.collector
version: 0.0.1
variables:
  config_file:
    description: "Newrelic otel collector configuration path"
    type: string
    required: false
    default: /etc/nr-otel-collector/config.yaml
  otel_exporter_otlp_endpoint:
    description: "Endpoint where NRDOT will send data"
    type: string
    required: false
    default: "otlp.nr-data.net:4317"
  new_relic_memory_limit_mib:
    description: "Memory limit for the NRDOT process"
    type: number
    required: false
    default: 100
deployment:
  on_host:
    executables:
      - path: /usr/bin/nr-otel-collector
        args: "--config=${config_file} --feature-gates=-pkg.translator.prometheus.NormalizeName"
        env: "OTEL_EXPORTER_OTLP_ENDPOINT=${otel_exporter_otlp_endpoint} NEW_RELIC_MEMORY_LIMIT_MIB=${new_relic_memory_limit_mib}"
        restart_policy:
          backoff_strategy:
            type: fixed
            backoff_delay_seconds: 5s
"#;

// NR_EBPF_AGENT_TYPE AgentType
pub(crate) const NR_EBPF_AGENT_TYPE: &str = r#"
namespace: newrelic
name: com.newrelic.nr_ebpf_agent
version: 0.0.1
variables:
deployment:
  on_host:
    executables:
      - path: /usr/bin/nr-ebpf-agent
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay_seconds: 5
"#;

// NR_EBPF_AGENT_CLIENT_TYPE AgentType
pub(crate) const NR_EBPF_AGENT_CLIENT_TYPE: &str = r#"
namespace: newrelic
name: com.newrelic.nr_ebpf_client
version: 0.0.1
variables:
deployment:
  on_host:
    executables:
      - path: /usr/bin/nr-ebpf-agent-client
    restart_policy:
      backoff_strategy:
        type: fixed
        backoff_delay_seconds: 5
"#;
