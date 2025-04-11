# Agent Control Configuration

Agent control will load the configuration from the file `config.yaml` in the corresponding directory (`/etc/newrelic-agent-control` or `/opt/homebrew/var/lib/newrelic-agent-control`).

Additionally, any configuration field can be set as an environment variable with the `NR_AC` prefix, using `__` to separate keys. Examples:

```bash
# Set log level to debug
NR_AC_LOG__LEVEL=DEBUG
# Set 'client_id' for the authentication config in the Fleet Control communication
NR_AC_FLEET_CONTROL__AUTH_CONFIG__CLIENT_ID="some-client-id"
```

## Configuration fields

### agents

List of agents configured locally. Check of [DEVELOPMENT](./DEVELOPMENT.md) for more information regarding agents and their corresponding local configuration.

Example:

```yaml
agents:
  nrdot: "newrelic/io.opentelemetry.collector:0.1.0"
```

### logs

Logs can be configured as follows:

```yaml
logs:
  format:
    target: true # Defaults to false, includes extra information for debugging purposes
    timestamp: "%Y-%m-%dT%H:%M%S" # Defaults to "%Y-%m-%dT%H:%M%S", details in <https://docs.rs/chrono/0.4.40/chrono/format/strftime/index.html#fn7>
    ansi_colors: true # Defaults to false, set up ansi-colors in the stdout logs output.
  level: debug # Defines the log level, defaults to "info".
  insecure_fine_grained_level: debug # Enables logs for external dependencies and sets its level. This cannot be considered secure since external dependencies may leek secrets. If this is set the 'level' field does not apply.
  file:
    enabled: true # Enabled logging to files.
    path: /some/path/logs.log # Optional path to write logs to, if not set it will use 'newrelic-agent-control.log' in the application logging directory.
```

### fleet_control

This configuration field enables and sets up the remote configuration features of Agent Control.

```yaml
fleet_control:
  endpoint: https://opamp.service.newrelic.com/v1/opamp # Fleet control endpoint.
  auth_config:
    token_url: https://system-identity-oauth.service.newrelic.com/oauth2/token # Endpoint to obtain access token
    client_id: "some-client-id" # Auth client id associated with the private key
    provider: "local" # Local auth provider which will load the provided key from 'private_key_path'
    private_key_path: "/private/key/path" # Path to the private key corresponding to the client-id.
    retries: 3 # Number of retries if the authentication fails.
  fleet_id: "some-id" # Fleet identifier.
  signature_validation:
    certificate_server_url: "https://newrelic.com/" # Server to obtain the certificate for signature validation.
    certificate_pem_file_path: "/some/certificate/" # Optional, if set it uses a local certificated instead of fetching it from 'certicate_server_url'.
    enabled: true # Defaults to true, allows disabling the signature validation.
```

### proxy

Agent Control will use the system proxy (configured through the standard `HTTP_PROXY` / `HTTPS_PROXY` environment variables) but
proxy options can also be configured using the proxy configuration field. If both are set, the precedence works as follows:

1. `proxy` configuration field
2. `HTTP_PROXY` environment variable
3. `HTTPS_PROXY` environment variable

⚠️ Proxy configuration is currently not compatible with fetching the certificate for signature validation. If you need to setup a proxy you will need to either use a local certificate through `fleet_control.signature_validation.certificate_pem_file_path` (recommended) or disable signature validation (highly discouraged).

```yaml
proxy:
  url: https://proxy.url:8080 # Proxy url in format '<protocol>://<user>:<password>@<host>:<port>'
  ca_bundle_dir: /some/dir # System path with CA certificates in PEM format (all '.pem' files in the directory will be read).
  ca_bundle_file: /some/pem/file # System path with CA certificates in PEM format.
  ignore_system_proxy: false # Default to false, if set to true HTTP_PROXY and HTTPS_PROXY environment variables will be ignored.
```

### server

Agent Control status server allows consulting the status of Agent Control and any controlled agent. It can be configured as follows:

```yaml
server:
  port: 51200 # Port for the status server. Defaults to 51200.
  host: "127.0.0.1" # Host for the status server. Defaults to '127.0.0.1'.
  enabled: true # The status server is enabled by default
```

### self_instrumentation

Agent Control can be configured to instrument itself and report traces, logs and metrics through OpenTelemetry. If proxy is configured globally it will also apply to self-instrumentation.

```yaml
self_instrumentation:
  opentelemetry:
    insecure_level: "newrelic_agent_control=info,off" # It is considered insecure because setting it up for external dependencies could potentially leak secrets. The default `newrelic_agent_control=debug,opamp_client=debug,off` disables external dependencies and can be considered secure.
    endpoint: https://otlp.nr-data.net:4318 # HTTPS endpoint to report instrumentation to.
    headers: {} # Headers that will be included in any request to the endpoint
    client_timeout: 10s # Timeout for performing requests, defaults to 30s.
    metrics:
      enabled: true # Defaults to false.
      interval: 120s # Interval to report metrics, it defaults to 60s.
    traces:
      enabled: true # Defaults to false.
      batch_config:
        scheduled_delay: 30s # Set the scheduled delay for batch export of traces. Defaults to 30s.
        max_size: 512 # Se the maximum number of traces to process in a single batch. Defaults to 512.
    logs:
      enabled: true # Defaults to false.
      batch_config:
        scheduled_delay: 30s # Set the scheduled delay for batch export of logs. Defaults to 30s.
        max_size: 512 # Se the maximum number of logs to process in a single batch. Defaults to 512.
```

### host_id

If the `host_id` is set it will be used to identify the host in Fleet Control instead of trying to fetch the identifier from the
host where Agent Control is running. Order of precedence:

1. Configured `host_id`.
2. Cloud instance id.
3. Machine id.

This applies for on-host environments only:

```yaml
host_id: "some-host-id" # Defaults to "" (no host set).
```

### k8s

The `k8s` configuration field applies for k8s environments only and are automatically set up through the corresponding helm chart:

```yaml
k8s:
  cluster_name: "some-cluster-name" # Required, used to identify the cluster in Fleet Control.
  namespace: "default" # Required, namespace where all resources managed by Agent Control will be created.
  chart_version: "0.0.50-dev" # Chart version used to deploy agent-control, it will be reported to Fleet Control.
```
