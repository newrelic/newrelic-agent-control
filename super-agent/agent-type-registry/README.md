# Agent Type Registry

Any `*.yaml` file defined in this folder or any subfolder will be embedded into the super agent binary
at compilation time. These files define the agent-type registry.

## Files organization recommendations

The following structure is recommended:

```
agent-type-registry/
├─ namespace/
│  ├─ agent_type_name-version.yaml
```

Example:

```
agent-type-registry/
├─ newrelic/
│  ├─ com.newrelic.infrastructure-0.1.3.yaml
│  ├─ io-opentelemetry.collector-0.2.0.yaml
```
