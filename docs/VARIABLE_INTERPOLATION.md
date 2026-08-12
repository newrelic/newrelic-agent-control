# Variable Interpolation in Agent Types

Agent Control uses a `${namespace:name}` template syntax to inject dynamic values into agent type
definitions. Variables are resolved at render time, just before a sub-agent is started or
reconfigured.

## Syntax

```
${<namespace>:<name>}
${<namespace>:<name>|<filter> [<filter> ...]}
```

Filters are optional post-processing steps applied to the resolved value (e.g. `indent 2`,
`to_upper`).

---

## Variable Reference

| Namespace | Prefix | Resolved from | Available on |
|---|---|---|---|
| Agent type variables | `nr-var` | `variables:` block in the agent type definition | On-host, K8s |
| Sub-agent attributes | `nr-sub` | AC-computed paths and identifiers for the sub-agent | On-host, K8s |
| Agent Control attributes | `nr-ac` | AC's own runtime attributes | On-host, K8s |
| Path helpers | `nr-path` | OS-native paths (avoids separator issues on Windows) | On-host |
| Environment variables | `nr-env` | Host environment (`std::env`) | On-host |
| Vault secrets | `nr-vault` | HashiCorp Vault (KV1 or KV2) | On-host |
| File secrets | `nr-file` | Local filesystem file contents | On-host |
| Kubernetes secrets | `nr-kubesec` | Kubernetes Secret objects | K8s |

---

### `nr-var` — Agent type variables

Values declared in the `variables:` block of the agent type definition. They can be provided by
Fleet Control or by a local config, and can have defaults.

```yaml
# agent type definition
variables:
  backoff_delay:
    type: string
    default: 20s

deployment:
  executables:
    - restart_policy:
        backoff_delay: ${nr-var:backoff_delay}
```

Nested variable names are flattened with `.`:

```yaml
variables:
  health_check:
    port:
      type: number
      default: 13133

# referenced as:
port: ${nr-var:health_check.port}
```

---

### `nr-sub` — Sub-agent attributes

Computed by AC for each managed sub-agent. Not configurable.

| Variable | Description |
|---|---|
| `nr-sub:agent_id` | The sub-agent's identifier string |
| `nr-sub:filesystem_agent_dir` | Dedicated filesystem directory for this agent |
| `nr-sub:shared_filesystem_dir` | Filesystem directory shared across all sub-agents |
| `nr-sub:remote_dir` | AC's remote data directory |
| `nr-sub:packages.<id>.dir` | Installation directory of the named OCI package |

```yaml
executables:
  - path: ${nr-sub:packages.nrdot.dir}/nrdot-collector
    env:
      AGENT_DIR: ${nr-sub:filesystem_agent_dir}
      SHARED: ${nr-sub:shared_filesystem_dir}
```

---

### `nr-ac` — Agent Control attributes

Runtime attributes from AC itself.

| Variable | Description |
|---|---|
| `nr-ac:host_id` | Unique identifier for the host running AC |

```yaml
env:
  OTEL_RESOURCE_ATTRIBUTES: "host.id=${nr-ac:host_id}"
```

---

### `nr-path` — Path helpers

Same value as `nr-sub:filesystem_agent_dir` but expressed as a native OS path. Use this instead
of `nr-sub:filesystem_agent_dir` inside values that are later used as filesystem paths, to avoid
path separator issues on Windows.

| Variable | Description |
|---|---|
| `nr-path:agent_dir` | Agent's dedicated filesystem directory (OS-native separator) |

```yaml
args:
  - --config
  - ${nr-path:agent_dir}/config.yaml
```

---

### `nr-env` — Environment variables

Reads a value directly from the host's environment at render time. No configuration required.

```yaml
env:
  NEW_RELIC_LICENSE_KEY: "${nr-env:NEW_RELIC_LICENSE_KEY}"
```

> **Note:** `nr-env` is a pass-through — it reads whatever is in the host environment. For secret
> management, prefer `nr-vault` or `nr-file`.

---

## Secrets Providers

Secret namespaces (`nr-env`, `nr-vault`, `nr-file`, `nr-kubesec`) are resolved on every remote
config update, not just at startup. This means secrets are refreshed automatically when a new
config is pushed from Fleet Control.

### `nr-vault` — HashiCorp Vault

Reads a value from a Vault KV secret. Requires `secrets_providers.vault` to be configured in
`agentcontrol.yml`.

**Secret path format:** `<source>:<mount>:<path>:<key>`

```yaml
# in agentcontrol.yml
secrets_providers:
  vault:
    sources:
      prod-vault:
        url: https://vault.example.com/v1
        token: s.xxxxxxxxx
        engine: kv2          # kv1 or kv2
      legacy-vault:
        url: https://old-vault.example.com/v1
        token: s.yyyyyyyyy
        engine: kv1
    client_timeout: 10s      # optional, default: 30s
```

```yaml
# in agent type definition
env:
  DB_PASSWORD: "${nr-vault:prod-vault:secret:database/credentials:password}"
  #                       ^source    ^mount ^path                ^key
```

Multiple sources can be defined under `sources`. Each source is identified by its key (e.g.
`prod-vault`, `legacy-vault`) and can target a different Vault cluster or engine version.

**Supported engines:**

| Engine | Description |
|---|---|
| `kv1` | KV Secrets Engine version 1 (no versioning) |
| `kv2` | KV Secrets Engine version 2 (versioned secrets) |

---

### `nr-file` — File secrets

Reads the contents of a local file. The file content is trimmed of leading/trailing whitespace.
No configuration needed in `agentcontrol.yml`.

**Secret path format:** absolute path to the file

```yaml
env:
  API_KEY: "${nr-file:/etc/newrelic/api.key}"
  CERT:    "${nr-file:/run/secrets/tls.crt}"
```

---

### `nr-kubesec` — Kubernetes secrets

Reads a key from a Kubernetes Secret object. Only available when AC is running on Kubernetes.
No configuration needed in `agentcontrol.yml`.

**Secret path format:** `<namespace>:<secret-name>:<key>`

```yaml
env:
  DB_PASSWORD: "${nr-kubesec:default:my-db-secret:password}"
  #                         ^ns     ^secret-name ^key
```

---

## Configuration reference for secrets providers

Only `vault` requires explicit configuration in `agentcontrol.yml`. The other providers are
always available.

```yaml
secrets_providers:
  vault:
    sources:
      <source-name>:
        url: <vault-url-including-/v1>   # required
        token: <vault-token>             # required
        engine: kv1 | kv2               # required
    client_timeout: <duration>           # optional (default: 30s)
```

| Field | Required | Description |
|---|---|---|
| `sources` | Yes | Map of named Vault sources |
| `sources.<name>.url` | Yes | Full Vault URL including `/v1` path |
| `sources.<name>.token` | Yes | Vault token for authentication |
| `sources.<name>.engine` | Yes | Secret engine version: `kv1` or `kv2` |
| `client_timeout` | No | HTTP timeout for Vault requests (default `30s`) |