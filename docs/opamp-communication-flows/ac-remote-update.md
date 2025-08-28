# Agent Control remote update

Agent Control installation will include the latest Agent Control version. In Kubernetes it will also include Flux 2.15.0.
These can be updated remotely.

Example message updating Agent Control version:

```yaml
agents:
  nr-infra: newrelic/com.newrelic.infrastructure:0.1.0
chart_version: "1.0.0"
```

Example message updating Flux version:

```yaml
agents:
  nr-infra: newrelic/com.newrelic.infrastructure:0.1.0
cd_chart_version: "1.0.0"
```

The message send through OpAMP will get transformed into an `AgentControlDynamicConfig`.

Agent Control then will update itself, if needed, to the received version. Check the [in-depth explanation](../ac-remote-update/how-it-works.md).
