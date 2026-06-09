# Agent Type Registry

Any `*.yaml` file defined in this folder or any subfolder will be embedded into the agent control binary
at compilation time. These files define the agent-type registry.

Each YAML file declares a top-level `protocol_version` (a quoted `MAJOR.MINOR` string identifying the
agent-type schema language, e.g. `"0.1"`), which is parsed separately and validated against the version
this Agent Control supports at ingestion time; see the
[agent type integration guide](../../docs/INTEGRATING_AGENTS.md#agent-type-metadata) for the
compatibility rules. The metadata then declares a `platform` (`host` or `kubernetes`) and, when
`platform: host`, also an `operating_system` (`linux` or `windows`). An agent that targets multiple
host operating systems and/or kubernetes is split
into one file per `(platform, operating_system)` pair, all sharing the same `namespace`,
`name` and `version`. At startup, Agent Control loads only the definitions whose `platform`
(and `operating_system`, when `platform: host`) match the binary it's running in: the on-host
binary loads `host` definitions matching its OS, and the Kubernetes binary loads `kubernetes`
definitions.

## Files organization recommendations

The following naming convention is recommended:

```
agent-type-registry/
├─ namespace/
│  ├─ <platform>[-<operating_system>]-<agent_type_name>-<version>.yaml
```

For `platform: host`, the operating system (`linux` or `windows`) is included; for
`platform: kubernetes` it is omitted.

Example:

```
agent-type-registry/
├─ newrelic/
│  ├─ kubernetes-com.newrelic.infrastructure-0.1.0.yaml
│  ├─ host-linux-com.newrelic.infrastructure-0.1.0.yaml
│  ├─ host-windows-com.newrelic.infrastructure-0.1.0.yaml
│  ├─ kubernetes-io.opentelemetry.collector-0.1.0.yaml
│  ├─ host-linux-io.opentelemetry.collector-0.1.0.yaml
```
