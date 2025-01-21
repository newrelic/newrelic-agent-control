# Agent Type Registry

Any `*.yaml` file defined in this folder or any subfolder will be embedded into the agent control binary
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
│  ├─ com.newrelic.infrastructure-0.1.0.yaml
│  ├─ io-opentelemetry.collector-0.1.0.yaml
```
