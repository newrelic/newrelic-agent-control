# supervisor

WIP universal supervisor, powered by OpAMP.

## Sample default static config (/tmp/static.yaml)

```yaml
op_amp: http://newserver.comm
agents:
  nr_otel_collector/gateway:
  nr_infra_agent:
    uuid_dir: /bin/sudoo
```

## Building the package

We can generate packages with the `goreleaser` tool, from the project root:

```console
goreleaser release [--clean] [--snapshot] # --snapshot if there are no git tags
```
