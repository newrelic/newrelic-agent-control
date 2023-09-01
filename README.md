# supervisor

WIP universal supervisor, powered by OpAMP.

## Sample default static config (/tmp/static.yaml)

```yaml
op_amp: http://newserver.comm
agents:
  nr_otel_collector_gw:
    agent_type: "newrelic/nrdot:0.1.0"
    values_file: "/path/to/user/nr_otel_collector_gw_values.yaml"
  nr_infra_agent:
    agent_type: "newrelic/infra_agent:1.47.0"
    values_file: "/path/to/user/nr_infra_agent_values.yaml
```
