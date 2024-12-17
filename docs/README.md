# Agent overview

New Relic agent control is a generic supervisor that can be configured to orchestrate  observability agents. It integrates with New Relic fleet control to help customers deploy, monitor and manage agents at scale. 

## Table of contents
- [Agent overview](#agent-overview)
  - [Table of contents](#table-of-contents)
  - [High-level architecture](#high-level-architecture)
    - [OpAMP](#opamp)
    - [Agent Types](#agent-types)
  - [Configuration](#configuration)
    - [Agent Control Configuration](#agent-control-configuration)
    - [Agent Values File](#agent-values-file)
    - [Configuration Persistence](#configuration-persistence)
    - [OpAMP Capabilities](#opamp-capabilities)
  - [Health](#health)
    - [Agents Health Reporting](#agents-health-reporting)
    - [Agent Control Health](#agent-control-health)
  - [Packages Download and Upgrade](#packages-download-and-upgrade)
  - [Running the agent](#running-the-agent)
    - [Running on-host](#running-on-host)
    - [Running in Kubernetes](#running-in-kubernetes)
      - [Prerequisites](#prerequisites)
      - [Steps](#steps)
  - [Troubleshooting](#troubleshooting)
  - [Testing](#testing)

## High-level architecture
![Agent Control Diagram](agent-control-diagram.png)

The Agent Control (SA) itself does not currently collect system or application telemetry itself. A combination of managed agents can be used to monitor your target entities and collect system and/or services telemetry. 

The SA has a modular architecture:
- The SA orchestrates observability **Agents** that need to be explicitly configured. We will see that agents are configured using an agent ID, **Agent Type** and agent type version. 
-  For each configured agent, the SA creates a **Supervisor** in charge of (1) orchestrating the agent based on provided configuration and (2) establishing the communication with the backend. 


### OpAMP

The **Open Agent Management Protocol** is "_...a network protocol for remote management of large fleets of data collection agents_" (from the [public specs](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md)). 

In a nutshell, OpAMP is the protocol handling the communication with the Fleet Management backend:
  - Agent Control registers itself as an agent.
  - Supervisors register agents.
  - Both receive remote configurations.
  - Both report health and status (metadata, effective configuration, …).
  - Both will receive package availability messages (not implemented).

Agents (including the agent control itself) support either `local` or `remote` configuration. Local configuration is expected to be deployed together with the SA. Remote configuration is centrally defined and managed via Fleet Management. 

### Agent Types

An Agent Type is a yaml based definition that determines how the Supervisor should manage a given agent. 

Agent Types are versioned to ensure compatibility with a given configuration values (no breaking changes). Agent Types define how agents get orchestrated using a set of `variables` and `deployment` settings for on-host and kubernetes scenarios.

This is a simplified version of the Infra Agent Type:

```yaml
# Agent Type Metadata (name + version)
namespace: newrelic
name: infra-agent
version: 1.0.0

# Variables configurable by the customers
variables:
  backoff_delay:
    description: "seconds until next retry if agent fails to start"
    type: string
    required: false
    default: 20s
  config_agent:
    description: "YAML config for the agent"
    type: file
    required: true
    file_path: "newrelic-infra.yml"
  config_integrations:
      description: "map of YAML configs for the OHIs"
      type: map[string]file
      required: false
      default: {}
      file_path: "integrations.d"

# How the agent should be supervised
deployment:
  on_host:
    executable:
      path: /opt/newrelic-agent-control/bin/newrelic-infra
      args: "--config=${nr-var:config_agent}"
      env: "NRIA_PLUGIN_DIR=${nr-var:config_integrations} NRIA_STATUS_SERVER_ENABLED=true"
      restart_policy:
        backoff_strategy:
          type: fixed
          backoff_delay: ${nr-var:backoff_delay}
      health:
        interval: 5s
        timeout: 5s
        http:
          path: "/v1/status"
          port: 8003
```

Note that the actual Infra Agent configuration `config_agent` is a variable whose yaml content is saved in a specific file defined by the Agent Type creator through a variable attribute `file_path`.

Current Agent Types can be found [here](../agent-control/agent-type-registry).

ℹ️ Refer to the [agent type](../agent-control/src/agent_type/README.md) implementation for the full definition of `variables` and `deployment` as well as a development guideline.

## Configuration

### Agent Control Configuration
Agent Control configuration defines which Agents need to be supervised.

The following Agent Control configuration example shows how to integrate the Infra Agent:

```yaml
# integrate with fleet control by defining the opamp backend settings
# remove to run the agent standalone (disconnected from fleet)
fleet_control:
  endpoint: https://opamp.service.newrelic.com/v1/opamp
  headers:
    api-key: YOUR_INGEST_KEY

# define agents to be supervised based on their agent types
# your-agent-id must contain 32 alpanumeric (or dashes) characters at most, and start and end with alphanumeric. 
# agents:
#   your-agent-id:
#     agent_type: "namespace/agent_type:version"
# 
agents:
  newrelic-infra:
    agent_type: "newrelic/infra-agent:0.1.0"
```

- `opamp` defines the required attributes to establish the connection with the backend.
- `agents` defines which agents should be running on the target environment. A built-in or custom agent type and version definition is expected. 

### Agent Values File

An agent supervised by the SA can be customized by defining or overriding those settings in a `values.yaml` (file or ConfigMap) provided when installed on a particular environment. 

The following values file shows how to configure the Infra Agent given the Agent Type we defined above:

```yaml
backoff_delay: 30s
config_agent: |
  license_key: 123456789
  log:
    level: debug
config_integrations: 
  nri-redis-example.yml: |
    integrations:
      - name: nri-redis
        env:
          hostname: localhost
          port: 6380
          keys: '{"0":["<KEY_1>"],"1":["<KEY_2>"]}'
          remote_monitoring: true    
```

- `backoff_delay` is a Supervisor setting that customers can tweak.
- `config_agent` and `config_integrations` are the actual agent configuration YAML files that the Agent Control stores for the Infra Agent to read.

### Configuration Persistence

This is the file structure:

```
├── etc
│   └── newrelic-agent-control
│       ├── agents.d
│       │   └── newrelic_infra
│       │       └── values
│       │           └── values.yaml
│       └── config.yaml
└── var
    └── lib
        └── newrelic-agent-control
           │── fleet
           │   ├── agents.d
           │   │   └── newrelic_infra
           │   │       └── values
           │   │           └── values.yaml
           │   └── config.yaml
           └── auto-generated
                └── agents.d
                    └── newrelic_infra
                        └── conf
                            ├── integrations.d
                            │   └── nri-redis.yaml
                            └── newrelic-infra.yaml
```

The Agent Control parses both its own configuration and agents values files to replace placeholders, and then SA then persists all these auto-generated files.

* Files under `/etc/newrelic-agent-control`  are used for local configuration. These are provisioned by the customer using Ansible like tools.
* Files under `/var/lib/newrelic-agent-control/fleet`  are used for remote configuration. These are centrally managed through New Relic fleet control, offering streamlined control for large-scale deployments.

The Agent Control generates actual agent configuration files and places these under `/var/lib/newrelic-agent-control/auto-generated` after processing Agent Type + Agent Values. 

### OpAMP Capabilities

Users can disable remote management just by commenting the `opamp` section in the  [Agent Control Configuration](#agent-control-configuration) file.

## Health

### Agents Health Reporting

Following OpAMP specs, each Supervisor sends an AgentToServer message to Fleet Management after any health change. 

The message includes a detailed ComponentHealth structure containing information such as the agent's health status, start time, last error, etc. 

On an unhealthy check, the Agent Control:
* Logs an error.
* Creates an internal data structure for health that follows the Opamp specs including:
  * A boolean is set to `true` if the agent is up and healthy.
  * `last_error` seen, which corresponds with the previously logged message.
  * A human readable `status` that takes the full response of the defined interface.
  * A timestamp.
* Compares this health data structure with the one from the last check. If it’s different in any way, sends an event to the Fleet Manager.

Agent Type creators can declare how the agent health exposes by using the health field in the definition. See the Infra Agent Type definition above as an example:

```yaml
#...
health:
  interval: 5s
  timeout: 5s
  http:
    path: "/v1/status"
    port: 8003
```

The Agent Control currently only supports a HTTP interface (just because this is how the Infra Agent and the OpenTelemetry Collector expose their status). More interfaces will be added as new agents with newer needs are integrated.

If the Agent Type does not declare its health interface, the Supervisor uses its restart policy violations as a fallback. In this case, an unhealthy message is sent when the maximum number of retries has been reached. 

In **Kubernetes**, we are leveraging health checks to its ecosystem because K8s already offers many built-in mechanisms to check the health of k8s objects. Therefore, the health information is obtained from the k8s objects related to each agent type. Currently, only the interval can be configured in the Agent Type, but we could offer the customer the possibility of selecting what information should be retrieved in the future.

ℹ️ Again, refer to the [agent type](../agent-control/src/agent_type/README.md) development guide to know more. 

### Agent Control Health

There is a service that ultimately exposes the /status endpoint for the Agent Control itself. This service performs a series of checks to determine the output (both in HTTP status code and message):
* Reachability of OpAMP endpoint (if OpAMP is enabled at all).
* Active agents and health of each one, in the same form as used by the OpAMP protocol, mentioned in the section above.

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

Users need to enable the local server by adding the following setting in the  [Agent Control Configuration](#agent-control-configuration) file:

```yaml
server:
    enabled: true
    # default values (change if needed)
    #host: "127.0.0.1"
    #port: 51200
```

## Packages Download and Upgrade

TBD

## Running the agent
The agent can be executed on-host (on-prem server, cloud compute instance, virtual machine, ...) or in a Kubernetes cluster.

### Running on-host

Compile and run locally:
1. Install [RUST](https://www.rust-lang.org/tools/install)
2. Run `cargo build --features onhost`
3. `newrelic-agent-control` binary will be generated at `./target/debug/newrelic-agent-control`
4. Prepare a `config.yaml` file in /etc/newrelic-agent-control/, example: 

```yaml
fleet_control:
  endpoint: https://opamp.service.newrelic.com/v1/opamp
  headers:
    api-key: YOUR_INGEST_KEY
agents:
  nr-otel-collector:
    agent_type: "newrelic/io.opentelemetry.collector:0.1.0"
```
5. Place values files in the folder `/etc/newrelic-agent-control/agents.d/{AGENT-ID}/` where `AGENT-ID` is a key in the `agents:` list. Example:
```yaml
config: |
    # the OTel collector config here
    # receivers:
    # exporters:
    # pipelines:
```
6. Execute the binary with the config file:  
    * `sudo ./target/debug/newrelic-agent-control`

### Running in Kubernetes

We use [Minikube](https://minikube.sigs.k8s.io/docs/) and [Tilt](https://tilt.dev/) to launch a local cluster and deploy the Agent Control [charts](https://github.com/newrelic/helm-charts/tree/master/charts/agent-control).

#### Prerequisites
- Install Minikube for local Kubernetes cluster emulation.
- Install [ctlptl](https://github.com/tilt-dev/ctlptl)
- Ensure you have Tilt installed for managing local development environments.
- Add a agent-control-deployment values file in `local/agent-control-deployment-values.yml`

Note: Adding the `'chart_repo'` setting, pointing to the [newrelic charts](https://github.com/newrelic/helm-charts/tree/master/charts) on a local path, allows to use local helm charts.
#### Steps
```shell
ctlptl create registry ctlptl-registry --port=5005
ctlptl create cluster minikube --registry=ctlptl-registry
make tilt-up
```

## Troubleshooting

See [diagnose issues with agent control logging](https://docs-preview.newrelic.com/docs/new-relic-agent-control#debug).

## Testing

Running the tests

Only for the feature on-host:
```
cargo test --features "onhost" -- --skip as_root
```

Only for the feature k8s:
```
cargo test --features "k8s"
```

Passing the flag --features "onhost, k8s" will throw a compilation error, there is a special feature "ci", that needs to be enabled to allow those 2 features at the same time (since we only want them together in specific CI scenarios).

[def]: #agent-overview

## Coverage

Generate coverage information easily by running the following `make` recipe from the root directory (will install `cargo-llvm-cov` if it's not installed already):

```console
make coverage
```

By default, this will generate a report in `lcov` format on `coverage/lcov.info` that IDEs such as VSCode can read via [certain extensions](https://marketplace.visualstudio.com/items?itemName=ryanluker.vscode-coverage-gutters). To modify the output format and the output location, use the variables `COVERAGE_OUT_FORMAT` and `COVERAGE_OUT_FILEPATH`:

```console
COVERAGE_OUT_FORMAT=json COVERAGE_OUT_FILEPATH=jcov-info.json make coverage
```
