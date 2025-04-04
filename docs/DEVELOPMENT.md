# Developing Agent Control

## Compiling and running Agent Control

As of now, Agent Control is supported on Linux (x86_64 and aarch64). The program is written in Rust, and for multiplatform compilation we leverage [`cross`](https://github.com/cross-rs/cross).

### On-host

To compile and run locally:

1. Install the [Rust toolchain](https://www.rust-lang.org/tools/install) for your system.
2. Run `cargo build --bin newrelic-agent-control-onhost`
3. `newrelic-agent-control-onhost` binary will be generated at `./target/debug/newrelic-agent-control-onhost`
4. Prepare a `config.yaml` file in `/etc/newrelic-agent-control/`, example:

    ```yaml
    fleet_control:
      endpoint: https://opamp.service.newrelic.com/v1/opamp
      headers:
        api-key: YOUR_INGEST_KEY
    agents:
      nr-otel-collector:
        agent_type: "newrelic/io.opentelemetry.collector:0.1.0"
    ```

5. Place values files in the folder `/etc/newrelic-agent-control/fleet/agents.d/{AGENT-ID}/` where `AGENT-ID` is a key in the
   `agents:` list. Example:

    ```yaml
    config: |
      # the OTel collector config here
      # receivers:
      # exporters:
      # pipelines:
    ```

6. Execute the binary with the config file with `sudo ./target/debug/newrelic-agent-control`

#### Filesystem layout and persistence

The following shows the directory structure used by Agent Control, assuming an existing sub-agent with the ID `newrelic-infra`:

```console
$ tree /
/
├── etc
│   └── newrelic-agent-control
│       ├── fleet
│       │   └── agents.d
│       │       └── newrelic-infra
│       │           └── values
│       │               └── values.yaml
│       └── config.yaml
└── var
    ├── lib
    │   └── newrelic-agent-control
    │      ├── fleet
    │      │   └── agents.d
    │      │       └── newrelic-infra
    │      │           └── values
    │      │               └── values.yaml
    │      ├── config.yaml
    │      └── auto-generated
    │          └── newrelic-infra
    │              ├── integrations.d
    │              │   └── nri-redis.yaml
    │              └── newrelic-infra.yaml
    └── log
        └── newrelic-agent-control
            ├── fleet
            │   └── agents.d
            │       └── newrelic-infra
            │           ├── stdout.log.2025-01-15-23
            │           └── stderr.log.2025-01-15-23
            └── newrelic-agent-control.log.2025-01-15-23
```

The directory `/etc/newrelic-agent-control` is used to store the **static** configs of AC and the values for its defined sub-agents, the latter inside the `fleet/agents.d` directory for each sub-agent. These files are expected to be put there and edited manually by the customer (or the installation process). When AC starts, these files are commonly read once, so any change to them would need an AC restart to actually enact a change in AC behavior.

The remote configurations and in general any files expected to dynamically change during AC execution are stored in `/var/lib/newrelic-agent-control`. Several kinds of transient files might be present there at any time, and AC might delete some of them (or all) when it first boots to start from a clean slate:

- The remote configurations, retrieved as is from FC, are stored respectively in `config.yaml` for AC and inside the `fleet/agents.d` directory for each sub-agent. Some other tracking information might be present, such as the remote config hashes or host identifiers, but these are implementation details that might change.
- The rendered files that are expected to be used by the sub-agent process directly (like configuration files for the New Relic Infrastructure Agent) will be added to the `auto-generated` directory, with a subdirectory being created for each sub-agent ID.

The directory inside `/var/log/newrelic-agent-control` will store the logs if file logging was configured, following a similar directory structure for AC and the sub-agents.

### Kubernetes

We use [`minikube`](https://minikube.sigs.k8s.io/docs/) and [`tilt`](https://tilt.dev/) to launch a local cluster and deploy the Agent Control [charts](https://github.com/newrelic/helm-charts/tree/master/charts/agent-control).

#### Prerequisites

- Install the [Rust toolchain](https://www.rust-lang.org/tools/install) for your system.
- Install `minikube` for local Kubernetes cluster emulation.
- Install [`ctlptl`](https://github.com/tilt-dev/ctlptl).
- Ensure you have `tilt` installed for managing local development environments.
- Add an Agent Control values file in `local/agent-control-tilt.yml`.

> [!CAUTION]
> Be aware that `cross` 0.2.5 [broke cross-compilation](https://github.com/cross-rs/cross/issues/1214). If you are using
> it, run the following command.
>
> ```sh
> docker pull ghcr.io/cross-rs/aarch64-unknown-linux-musl:0.2.5 --platform linux/x86_64
> ```

Note: Adding the `'chart_repo'` setting, pointing to the [New Relic charts](https://github.com/newrelic/helm-charts/tree/master/charts) on a local path, allows using local helm charts.

#### Steps

```sh
ctlptl create registry ctlptl-registry --port=5005
ctlptl create cluster minikube --registry=ctlptl-registry
make tilt-up
```

## Troubleshooting

See [diagnose issues with agent control logging](https://docs-preview.newrelic.com/docs/new-relic-agent-control#debug).

### Disable Fleet Control

Users can disable remote management just by commenting its configuration out from `/etc/newrelic-agent-control/config.yaml` (on-host):

```yaml
# fleet_control:
#   endpoint: https://opamp.service.newrelic.com/v1/opamp
#   headers:
#     api-key: API_KEY_HERE
#   fleet_id: FLEET_ID_HERE
#   auth_config:
#     token_url: PLACEHOLDER
#     client_id: PLACEHOLDER
#     provider: PLACEHOLDER
#     private_key_path: PLACEHOLDER
```

Or by placing `enabled: false` under the `fleet_control` section in the Agent Control configuration values (k8s):

```yaml
# For K8s, inside the Helm values:
agent-control-deployment:
  image:
    imagePullPolicy: Always
  config:
    fleet_control:
      enabled: false
  # ...
```

### Agent Control Health

There is a service that ultimately exposes a `/status` endpoint for Agent Control itself. This service performs a series of checks to determine the output (both in HTTP status code and message):

- Reachability of Fleet Control endpoint (if Fleet Control is enabled at all).
- Active agents and health of each one, in the same form as used by the OpAMP protocol, mentioned when discussing [sub-agent health](./INTEGRATING_AGENTS.md#health-status).

```json
{
  "agent_control": {
    "healthy": true
  },
  "opamp": {
    "enabled": true,
    "endpoint": "https://opamp.service.newrelic.com/v1/opamp",
    "reachable": true
  },
  "sub_agents": {
    "nr-otel-collector": {
      "agent_id": "nr-otel-collector",
      "agent_type": "newrelic/io.opentelemetry.collector:0.1.0",
      "healthy": true
    },
    "nr-infra-agent": {
      "agent_id": "nr-infra-agent",
      "agent_type": "newrelic/com.newrelic.infrastructure:0.1.1",
      "healthy": false,
      "last_error": "process exited with code: exit status: 1"
    }
  }
}
```

Users need to enable the local server by adding the following setting in the Agent Control configuration file:

```yaml
server:
    enabled: true
    # default values (change if needed)
    #host: "127.0.0.1"
    #port: 51200
```

For Kubernetes, the status endpoint is enabled by default. You can access this easily by performing a Kubernetes [port-forward](https://kubernetes.io/docs/tasks/access-application-cluster/port-forward-access-application-cluster/), using the following commands **on separate shells**:

```console
$ kubectl port-forward ac-agent-control-6558446569-rtwh4 -n newrelic 51200:51200
Forwarding from 127.0.0.1:51200 -> 51200
Forwarding from [::1]:51200 -> 51200

$ curl localhost:51200/status | jq
# ... contents will appear here formatted and highlighted
```

## Testing

### General

```sh
cargo test --workspace --exclude 'newrelic_agent_control' --all-targets
```

We have `Makefile`s containing targets for testing. [Inspect them](../agent-control/Makefile) for more details.

### Feature `onhost`

Running tests for the agent control lib excluding root-required tests (on-host)

```sh
make -C agent-control test/onhost
```

Run tests agent control integration tests excluding root-required tests.

```sh
make -C agent-control test/onhost/integration
```

#### Tests that require root user

Running tests that require root user can be not straight-forward, as the Rust toolchain installers like `rustup` tend to not install them globally on a system, so doing `sudo cargo` won't work. An easy way to run the root-required tests is spinning up a container where the user is root and running them there with:

```sh
make -C agent-control test/onhost/root
```

### Feature `k8s`

Running basic tests, not requiring an existing Kubernetes cluster.

```sh
make -C agent-control test/k8s
```

#### Tests that require an existing Kubernetes cluster

```sh
make -C agent-control test/k8s/integration
```

## Coverage

Generate coverage information easily by running the following `make` recipe from the root directory (will install `cargo-llvm-cov` if it's not installed already):

```console
make coverage
```

By default, this will generate a report in `lcov` format on `coverage/lcov.info` that IDEs such as VSCode can read via [certain extensions](https://marketplace.visualstudio.com/items?itemName=ryanluker.vscode-coverage-gutters). To modify the output format and the output location, use the variables `COVERAGE_OUT_FORMAT` and `COVERAGE_OUT_FILEPATH`:

```console
COVERAGE_OUT_FORMAT=json COVERAGE_OUT_FILEPATH=jcov-info.json make coverage
```

## Additional information

We maintain separate directories for other documented topics under this `docs` directory and in other Markdown files throughout the codebase. The latter will be centralized under the `docs` directory over time. Feel free to check these documents and ask doubts or propose changes!
