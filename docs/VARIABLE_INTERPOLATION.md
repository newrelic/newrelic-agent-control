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
| Azure Key Vault secrets | `nr-azurekv` | Azure Key Vault (Managed Identity or Service Principal) | On-host |
| File values | `nr-file` | Local filesystem file contents | On-host |
| Kubernetes secrets | `nr-kubesec` | Kubernetes Secret objects | K8s |
| Kubernetes ConfigMaps | `nr-kubecm` | Kubernetes ConfigMap objects | K8s |

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

## Value Providers

`nr-env`, `nr-vault`, `nr-azurekv`, `nr-file`, `nr-kubesec`, and `nr-kubecm` are resolved by a
value provider on every remote config update, not just at startup. This means their values are
refreshed automatically when a new config is pushed from Fleet Control.

Value providers are configured under the `value_providers:` key in `agentcontrol.yml` (the legacy
key `secrets_providers:` is still accepted as an alias).

### `nr-vault` — HashiCorp Vault

Reads a value from a Vault KV secret. Requires `value_providers.vault` to be configured in
`agentcontrol.yml`.

**Secret path format:** `<source>:<mount>:<path>:<key>`

```yaml
# in agentcontrol.yml
value_providers:
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

### `nr-azurekv` — Azure Key Vault

Reads a secret value from an Azure Key Vault instance. Requires `value_providers.azure_key_vault`
to be configured in `agentcontrol.yml`.

**Secret path format:** `<secret-name>`

```yaml
# in agent type definition
env:
  LICENSE_KEY: "${nr-azurekv:newrelic-license-key}"
  #                          ^secret name in the vault
```

Two authentication methods are supported. Choose the one that matches your deployment environment.

#### Managed Identity

Use this when Agent Control runs on Azure infrastructure (Azure VM or AKS pod) with a Managed
Identity assigned. No credentials are stored in the config, the identity is attached to the
compute resource at the infrastructure level.

```yaml
# in agentcontrol.yml
value_providers:
  azure_key_vault:
    vault_url: https://my-vault.vault.azure.net/
    auth:
      type: managed_identity   # this is the default; the auth block can be omitted entirely
```

#### Service Principal

Use this when Agent Control runs outside Azure (on-prem, other cloud providers) or in any
environment where Managed Identity is unavailable.

```yaml
# in agentcontrol.yml
value_providers:
  azure_key_vault:
    vault_url: https://my-vault.vault.azure.net/
    auth:
      type: service_principal
      tenant_id: "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
      client_id: "yyyyyyyy-yyyy-yyyy-yyyy-yyyyyyyyyyyy"
      client_secret: "your-client-secret"
```

### `nr-file` — File values

Reads the contents of a local file. The file content is trimmed of leading/trailing whitespace.
No configuration needed in `agentcontrol.yml`.

**Value path format:** absolute path to the file

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

### `nr-kubecm` — Kubernetes ConfigMaps

Reads a key from a Kubernetes ConfigMap object. Only available when AC is running on Kubernetes.
No configuration needed in `agentcontrol.yml`.

**Value path format:** `<namespace>:<configmap-name>:<key>`

```yaml
env:
  LOG_LEVEL: "${nr-kubecm:default:my-app-config:log-level}"
  #                      ^ns     ^configmap-name ^key
```

---

## Configuration reference for value providers

Only `vault` and `azure_key_vault` require explicit configuration in `agentcontrol.yml`. The
other providers are always available.

### Vault

```yaml
value_providers:
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

### Azure Key Vault

```yaml
value_providers:
  azure_key_vault:
    vault_url: <azure-key-vault-url>     # required
    auth:                                # optional (default: managed_identity)
      type: managed_identity
      # or:
      # type: service_principal
      # tenant_id: <tenant-id>
      # client_id: <client-id>
      # client_secret: <client-secret>
    client_timeout: <duration>           # optional (default: 30s)
```

| Field | Required | Description |
|---|---|---|
| `vault_url` | Yes | Azure Key Vault endpoint URL (e.g. `https://my-vault.vault.azure.net/`) |
| `auth.type` | No | Authentication method: `managed_identity` (default) or `service_principal` |
| `auth.tenant_id` | If `service_principal` | Azure tenant (directory) ID |
| `auth.client_id` | If `service_principal` | Service principal application (client) ID |
| `auth.client_secret` | If `service_principal` | Service principal client secret |
| `client_timeout` | No | HTTP timeout for requests (default `30s`) |