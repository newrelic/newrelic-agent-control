# Agent Control remote update

Agent Control can be updated remotely via Fleet Control. The mechanism differs by environment.

## On-host

Fleet Control pushes a `version` field. Absent or empty means no update.

Example message updating the on-host Agent Control binary:

```yaml
agents:
  nr-infra: newrelic/com.newrelic.infrastructure:0.1.0
version: "v1.2.3"
```

Example with a pinned digest:

```yaml
agents:
  nr-infra: newrelic/com.newrelic.infrastructure:0.1.0
version: "v1.2.3@sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
```

## Kubernetes

Agent Control installation will include the latest Agent Control version. In Kubernetes it will also include Flux 2.15.0.

Example message updating Agent Control chart version:

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

Example message updating both versions:

```yaml
agents:
  nr-infra: newrelic/com.newrelic.infrastructure:0.1.0
chart_version: "1.0.0"
cd_chart_version: "1.0.0"
```

The message sent through OpAMP will get transformed into an `AgentControlDynamicConfig`.

Agent Control then will update itself, if needed, to the received version. Check the [in-depth explanation](../ac-remote-update/how-it-works.md).
