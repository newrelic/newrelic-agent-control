# supervisor

WIP universal supervisor, powered by OpAMP.

## Sample default static config (/tmp/static.yaml)

```yaml
opamp:
  endpoint: https://opamp.service.newrelic.com/v1/opamp
  headers:
    api-key: API_KEY_HERE

 agents:
  nr_infra_agent:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.1"
  nr_otel_collector:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
```
