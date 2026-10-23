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
| AWS Secrets Manager secrets | `nr-awssm` | AWS Secrets Manager (access key or EC2 instance profile) | On-host |
| Google Cloud Secret Manager secrets | `nr-gcpsm` | Google Cloud Secret Manager (metadata server or service account key) | On-host |
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

`nr-env`, `nr-vault`, `nr-azurekv`, `nr-awssm`, `nr-gcpsm`, `nr-file`, `nr-kubesec`, and `nr-kubecm` are resolved by a
value provider on every remote config update, not just at startup. This means their values are
refreshed automatically when a new config is pushed from Fleet Control.

Value providers are configured under the `value_providers:` key in the Agent Control configuration (the legacy
key `secrets_providers:` is still accepted as an alias).

### `nr-vault` — HashiCorp Vault

Reads a value from a Vault KV secret. Requires `value_providers.vault` to be configured in the Agent Control configuration.

**Secret path format:** `<source>:<mount>:<path>:<key>`

```yaml
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
to be configured in the Agent Control configuration. Multiple named sources can be configured to target
different vaults.

**Secret path format:** `<source-name>:<secret-name>`

```yaml
# in agent type definition
env:
  LICENSE_KEY: "${nr-azurekv:prod-vault:newrelic-license-key}"
  #                          ^source    ^secret name in the vault
```

Two authentication methods are supported per source. Choose the one that matches your deployment environment.

#### Managed Identity

Use this when Agent Control runs on Azure infrastructure (Azure VM or AKS pod) with a Managed
Identity assigned. No credentials are stored in the config, the identity is attached to the
compute resource at the infrastructure level.

```yaml
# in AC configuration
value_providers:
  azure_key_vault:
    sources:
      prod-vault:
        vault_url: https://my-vault.vault.azure.net/
        auth:
          type: managed_identity   # this is the default; the auth block can be omitted entirely
```

#### Service Principal

Use this when Agent Control runs outside Azure (on-prem, other cloud providers) or in any
environment where Managed Identity is unavailable.

```yaml
# in AC configuration
value_providers:
  azure_key_vault:
    sources:
      prod-vault:
        vault_url: https://my-vault.vault.azure.net/
        auth:
          type: service_principal
          tenant_id: "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
          client_id: "yyyyyyyy-yyyy-yyyy-yyyy-yyyyyyyyyyyy"
          client_secret: "your-client-secret"
```

### `nr-awssm` — AWS Secrets Manager

Reads a string secret from AWS Secrets Manager. Requires `value_providers.aws_secrets_manager`
to be configured in the Agent Control configuration. Multiple named sources can be configured to target
different regions or accounts.

**Secret path format:** `<source-name>:<secret-id>`, where the secret id is the secret name or its ARN.

```yaml
# in agent type definition
env:
  LICENSE_KEY: "${nr-awssm:prod:newrelic/license-key}"
  #                        ^source ^secret name or ARN
```

Binary secrets are not supported. The IAM identity needs `secretsmanager:GetSecretValue` on the
secret (and `kms:Decrypt` if it is encrypted with a customer managed key).

#### Instance profile

Use this when Agent Control runs on an EC2 instance with an IAM role attached. No credentials are
stored in the config, they are retrieved from IMDSv2 and refreshed before they expire.

```yaml
# in AC config
value_providers:
  aws_secrets_manager:
    sources:
      prod:
        region: us-east-1
        auth:
          type: instance_profile   # this is the default; the auth block can be omitted entirely
```

#### Static credentials

Use this when Agent Control runs outside AWS or where no instance profile is available. Use the
access key of an IAM user: temporary credentials (with a session token) are not supported because
they are read once from the configuration and would expire while Agent Control is running.

```yaml
# in AC config
value_providers:
  aws_secrets_manager:
    sources:
      prod:
        region: us-east-1
        auth:
          type: access_key
          access_key_id: "AKIAXXXXXXXXXXXXXXXX"
          secret_access_key: "your-secret-access-key"
```

### `nr-gcpsm` — Google Cloud Secret Manager

Reads a text secret from Google Cloud Secret Manager. Requires `value_providers.gcp_secret_manager`
to be configured in the Agent Control configuration. Multiple named sources can be configured to target
different projects.

**Secret path format:** `<source-name>:<secret-name>:<version>`. The version is required and is a number
or `latest`.

```yaml
# in agent type definition
env:
  LICENSE_KEY: "${nr-gcpsm:prod:newrelic-license-key:latest}"
  #                        ^source ^secret name       ^version
  PINNED_KEY: "${nr-gcpsm:prod:newrelic-license-key:3}"
```

Secret names and versions can only contain letters, digits, `-` and `_`. Binary secrets are not
supported. The identity needs the `roles/secretmanager.secretAccessor` role on the secret.

#### Metadata server

Use this when Agent Control runs on Google Cloud with a service account attached (Compute Engine
VM, GKE with Workload Identity, etc.). No credentials are stored in the config, access tokens are
retrieved from the metadata server.

```yaml
# in AC config
value_providers:
  gcp_secret_manager:
    sources:
      prod:
        project: my-project
        auth:
          type: metadata_server   # this is the default; the auth block can be omitted entirely
```

#### Service account key

Use this when Agent Control runs outside Google Cloud or where no service account is attached.
Point `key_file` to the JSON key file downloaded from Google Cloud. It is read once at startup,
so restart Agent Control after rotating the key. Restrict the file permissions to the user running
Agent Control.

```yaml
# in AC config
value_providers:
  gcp_secret_manager:
    sources:
      prod:
        project: my-project
        auth:
          type: service_account
          key_file: /etc/newrelic-agent-control/gcp-key.json
```

### `nr-file` — File values

Reads the contents of a local file. The file content is trimmed of leading/trailing whitespace.
No configuration needed in the Agent Control configuration.

**Value path format:** absolute path to the file

```yaml
env:
  API_KEY: "${nr-file:/etc/newrelic/api.key}"
  CERT:    "${nr-file:/run/secrets/tls.crt}"
```

---

### `nr-kubesec` — Kubernetes secrets

Reads a key from a Kubernetes Secret object. Only available when AC is running on Kubernetes.
No configuration needed in the Agent Control configuration.

**Secret path format:** `<namespace>:<secret-name>:<key>`

```yaml
env:
  DB_PASSWORD: "${nr-kubesec:default:my-db-secret:password}"
  #                         ^ns     ^secret-name ^key
```

---

### `nr-kubecm` — Kubernetes ConfigMaps

Reads a key from a Kubernetes ConfigMap object. Only available when AC is running on Kubernetes.
No configuration needed in the Agent Control configuration.

**Value path format:** `<namespace>:<configmap-name>:<key>`

```yaml
env:
  LOG_LEVEL: "${nr-kubecm:default:my-app-config:log-level}"
  #                      ^ns     ^configmap-name ^key
```

---

## Configuration reference for value providers

Only `vault`, `azure_key_vault`, `aws_secrets_manager` and `gcp_secret_manager` require explicit configuration in the Agent Control configuration. The
other providers are always available.

For the full field reference of each provider, see [`docs/CONFIG.md#value_providers`](CONFIG.md#value_providers).
