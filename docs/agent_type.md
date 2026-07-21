# Agent Type Overview

Agent Type Definition is a YAML file that defines an agent's configuration and behavior. It consists of three main sections: metadata, deployment, and variables.

By defining these three sections, developers can create a customizable and flexible agent type that can be used in various environments.

On top of those sections, every file declares a top-level [`protocol_version`](#protocol-version) that versions the schema language the file is written against.

## Protocol version

`protocol_version` is a top-level field — separate from the three sections below — that declares the version of the agent-type **schema language itself**: the set of fields and their meaning that Agent Control knows how to parse, *including the shape of the metadata*. It is decoupled from both the agent type `version` (the definition's semver) and the Agent Control release version.

It is a quoted `MAJOR.MINOR` string (e.g. `"1.0"`). The value **must be quoted**, otherwise YAML parses `0.1` as a float and the field is rejected.

It is parsed and validated on its own, at the registry ingestion boundary, *before* the rest of the document is interpreted — so it can gate files whose metadata or other sections use a shape this Agent Control would not otherwise understand. Each Agent Control release understands a single maximum protocol version, and the `protocol_version` is treated as a single ordered `MAJOR.MINOR` value. The compatibility rules are:

* Newer than supported (higher `major`, or same `major` with a higher `minor`): rejected — the file is newer than this Agent Control understands.
* Equal to or older than supported: accepted — Agent Control understands every protocol version up to and including the supported one.

For example, an Agent Control supporting `1.6` accepts everything up to `1.6` (including `0.9` and `1.0`..=`1.6`) and rejects anything newer (`1.7`, `2.0`, ...).

## Metadata

The metadata section contains information about the agent type: its `name`, `version`, `namespace`, and the target platform.

```yaml
namespace: newrelic
name: com.newrelic.opentelemetry.collector
version: 0.0.1
platform: host
operating_system: linux
```

```yaml
namespace: newrelic
name: com.newrelic.opentelemetry.collector
version: 0.0.1
platform: kubernetes
```

* The name and namespace must:
  * Start with an ASCII letter and end with a letter or digit.
  * Only contain lowercase letters, digits, `.` or `_` (note: `-` is **not** allowed).
  * Be at most 64 characters long.
* The version must be a plain `Major.Minor.Patch` semver (e.g. `0.1.0`). Pre-release
  (`-alpha.1`) and build-metadata (`+build`) suffixes are **not** allowed, and the version
  must be at most 14 characters long (keeps the derived OCI tag bounded).
* `platform`: the target platform. One of `host` or `kubernetes`.
* `operating_system`: required when `platform: host`. One of `linux` or `windows`. Must be omitted for `platform: kubernetes`.

The `platform` (and `operating_system` when applicable) drives how the rest of the document is parsed: the `deployment` block is dispatched to the on-host or Kubernetes deserializer based on these fields.

## Variables

The `variables` section allows developers to define variables that end users can set. These variables can adjust the agent's or system's configuration.

```yaml
variables:
  config_agent:
    description: "Newrelic infra configuration"
    type: yaml
    required: false
    default: {}
  backoff_delay:
    description: "seconds until next retry if agent fails to start"
    type: string
    required: false
    variants: [5s, 10s, 20s, 30s]
    default: 20s
  enable_file_logging:
    description: "enable logging the on host executables' logs to files"
    type: bool
    required: false
    default: false
```

Nested variable names are supported. For instance:

```yaml
variables:
  log:
    level:
      description: "Log level with only info and error"
      type: string
      required: false
      default: info
      variants: ["info", "error"]
```

All variables have a few common attributes:

* `description`: A brief description of the variable. This is useful for documentation purposes and can help others understand the purpose of the variable.
* `type`: The data type of the variable. We support several data types, including `string`, `file`, `bool`, `yaml`, and more.
* `variants`: Represents a defined list of acceptable values for the variable. Only values present in the variants list are considered valid.
* `default`: The default value for the variable if no value is provided.
* `required`: Whether the variable is mandatory to be provided or not.

In terms of variable types, we currently support the following types listed in [this source file](./variable/variable_type.rs#L22):

* `string`: A string value, such as "Hello, world!"
* `number`: An numeric value, such as 42 or 0.25
* `boolean`: A boolean value, which can be either *true* or *false*
* `yaml`: The YAML type variable is used to handle multi-line strings that will be parsed as YAML such as Helm Charts values.
* `map[string]yaml`: Handles YAML values that guarantee their top-level fields are strings. Useful for defining file system entries for on-host.

## Deployment

The deployment section indicates how the agent should be executed and how its health should be checked.

Note you can reference the variables defined in the `variables` section using `${nr-var:variable_name}`. And this is valid for nested variables as well: following the example above, you would be able to use `${nr-var:log.info}`.

### Template Functions

You can enhance templated variables by applying functions to them, enabling transformations as needed.

Functions are pipelined, meaning the output of each transformation serves as the input for the next one: `${nr-var:variable_name | func1 | func2 | ... | funcN}`.

#### Indent(n)

The `indent` function indents each new line with `n` spaces. Essentially, it adds `n` spaces after each `\n`. For example, `${nr-var:key|indent 2}` will prepend 2 spaces to the beginning of each line in the string produced by the variable.

This is particularly useful when rendering YAML inside a multiline string where the YAML being rendered requires specific indentation, as shown below:

```yaml
multi_line_string: |
  fixed_key:
    ${nr-var:yaml_variable | indent 2 }
```

### On Host Deployment

For on-host deployment (`platform: host`, `operating_system: linux` or `windows`), use the following format:

```yaml
deployment:
  enable_file_logging: ${nr-var:enable_file_logging}
  health:
    interval: 5s
    timeout: 5s
    checks:
      - kind: Process
      - kind: Http
        path: "/v1/status"
        port: 8003
  executables:
    - id: newrelic-infra
      path: /usr/bin/newrelic-infra
      args:
        - --config
        - ${nr-var:config_agent}
      env: "NRIA_PLUGIN_DIR="${nr-sub:shared_filesystem_dir}/infra-agent-ohi-configs" NRIA_STATUS_SERVER_ENABLED=true"
      restart_policy:
        backoff_strategy:
          type: fixed
          backoff_delay: ${nr-var:backoff_delay}
```

In this section:

* `enable_file_logging`: This setting turns on logging for the agent supervisor
* `health`: The measures used to check the health status of the agent.
* `executables`: This outlines the list of binaries the agent supervisor runs. Developers can define:
  * * `id`: Unique identifier for the exec used by the health checker.
    * `path`: The location of the binary required.
    * `args`: The command-line arguments needed by the binary.
    * `env`: Specifies the required environment variables.
    * `restart_policy`: The guidelines for if or when the process should be restarted.

These diverse options offer extensive customization for your agent's deployment.

#### Restart Policy

`restart_policy` provides a set of instructions on how and when the agent process should be restarted. It's crucial for maintaining the agent's availability and reliability, particularly in case of unexpected failures or problems.

In the `backoff_strategy` we have:

* `type`: This field can take several forms - `fixed`, `linear`, or `exponential`. It determines the delay timing strategy between retries.
  * `fixed`: Constant delay interval between retries. This is the default type.
  * `linear`: Delay interval increases linearly after each retry.
  * `exponential`: Delay interval doubles after each retry.
* `backoff_delay`: It defines the duration between retries when a restart is needed. This delay protects against aggressive restarts. Default is *2s*.
* `max_retries`: This integer value defines the maximum number of retry attempts before exiting the retry mechanism and accepting the failure. Default is *0*.
* `last_retry_interal`: This is used to store the duration of the last delay. It can especially be relevant in case of *linear* or *exponential* back-off strategies where each retry level has a different delay value. Default is *600*.

#### On Host Health

The `health` section in the deployment configuration is where you can specify how to monitor the health status of the agent. This is critical for maintaining the reliability of your agent and ensuring that it's functioning correctly. It uses an explicit `checks:` list where every entry is discriminated by an explicit `kind:` field:

```yaml
health:
    interval: 5s
    timeout: 5s
    checks:
      - kind: Process
      - kind: Http
        path: "/v1/status"
        port: 8003
        healthy_status_codes: [200, 203, 204]
```

In this configuration:

* `interval`: This parameter specifies the frequency at which health checks should be performed.
* `timeout`: This is the maximum time the agent should wait for an HTTP health check response.
* `checks`: The explicit list of checks to run. Empty (or omitted) means health reporting is disabled. Any single unhealthy check makes the sub-agent unhealthy.
  * `kind: Process` surfaces the health of the supervised executable. No parameters. Only meaningful when the agent type declares `executables`.
  * `kind: Http` polls an HTTP endpoint. Fields: `host` (default `127.0.0.1`), `path`, `port`, `headers`, `healthy_status_codes` (empty means the 2xx range is treated as healthy).
  * `kind: File` reads a health-status file. Field: `path`.

By finely tuning these parameters, developers can closely monitor the agent's performance and address issues instantly. Adopting a robust health check strategy helps minimize downtime and keeps your system resilient and reliable.

Additionally, alternate protocols and interfaces can be mentioned under `health` - for instance, a `cmd` interface to run a command or script, or a `file` interface to read a specific file for agent status. However, as of current updates, these methods are **not implemented** yet.

```yaml
# ...
health:
  interval: 30s
  timeout: 5s
  cmd:
    command: "newrelic-agent-control --status"
    healthy_codes: [0] 
    unhealthy_string: ".*(unhealthy|fatal|error).*"
```

```yaml
# ...
health:
  interval: 30s
  timeout: 5s
  file:
    path: "/etc/newrelic-infra/health.lock"
    should_be_present: true
    unhealthy_string: ".*(unhealthy|fatal|error).*"
```

#### On Host Version

On-host agents do not define a version-check command. The agent version reported to Fleet Control
(the `agent.version` identifying attribute) is derived from the OCI **package** the agent is
deployed from, using the version configured under
`deployment.packages.<package_id>.download.oci.version`:

```yaml
deployment:
  packages:
    newrelic-infra:
      download:
        oci:
          repository: newrelic-infra
          version: ${nr-var:package_version}
```

The resolved value of that `version` field is published as `agent.version`.

When an agent type defines **more than one package**, the package to report is selected explicitly
with the top-level `deployment.reported_version_package` field, which must name one of the declared
packages:

```yaml
deployment:
  reported_version_package: newrelic-infra   # required when more than one package is declared
  packages:
    newrelic-infra:
      download:
        oci:
          repository: newrelic-infra
          version: ${nr-var:package_version}
    nri-flex:
      download:
        oci:
          repository: nri-flex
          version: ${nr-var:flex_version}
```

Rules:

* With **one** package, `reported_version_package` is optional and defaults to that sole package.
* With **more than one** package, `reported_version_package` is **required** and must reference a declared
  package id; otherwise the agent type fails validation at parse time.
* With **no** packages, no `agent.version` is reported.

See [On Host Packages](#on-host-packages) for the full package configuration.

### Kubernetes Deployment

The Agent Control leverages [Flux](https://fluxcd.io/) to act as an operator running Helm commands (install, upgrade, delete) as needed based on the provided configurations.

Then, for a Kubernetes deployment (`platform: kubernetes`), we use the following format:

```yaml
deployment:
  # See com.newrelic.infrastructure Agent type for description of fields.
  health:
    interval: 30s
  objects:
    repository:
      apiVersion: source.toolkit.fluxcd.io/v1
      kind: HelmRepository
      metadata:
        name: ${nr-sub:agent_id}
      spec:
        interval: 30m
        provider: generic
        url: https://helm-charts.newrelic.com
    release:
      apiVersion: helm.toolkit.fluxcd.io/v2
      kind: HelmRelease
      metadata:
        name: ${nr-sub:agent_id}
      spec:
        interval: 3m
        chart:
          spec:
            chart: nr-k8s-otel-collector
            version: ${nr-var:chart_version}
            sourceRef:
              kind: HelmRepository
              name: ${nr-sub:agent_id}
            interval: 3m
        install:
          disableWait: true
          disableWaitForJobs: true
          replace: true
        upgrade:
          disableWait: true
          disableWaitForJobs: true
          cleanupOnFail: true
          force: true
        values:
          ${nr-var:chart_values}
```

#### Kubernetes Objects

##### Repository

This is the K8s object whose kind is *HelmRepository*. It contains all the info to retrieve Helm charts.

```yaml
apiVersion: source.toolkit.fluxcd.io/v1
kind: HelmRepository
metadata:
  name: open-telemetry
  namespace: default
spec:
  interval: 1m
  url: https://open-telemetry.github.io/opentelemetry-helm-charts
```

Visit [Flux API reference for v1](https://fluxcd.io/flux/components/source/api/v1/#source.toolkit.fluxcd.io/v1.HelmRepository) for detail spec information.

##### Release

This is the K8s object whose kind is *HelmRelease*. It has all the data to deploy a chart (version, values, tests, post-install actions).

```yaml
apiVersion: helm.toolkit.fluxcd.io/v2
kind: HelmRelease
metadata:
  name: otel-collector
  namespace: default
spec:
  interval: 1h0m0s
  chart:
    spec:
      chart: opentelemetry-collector
      version: '>=0.60.0 <1.0.0'
      sourceRef:
        kind: HelmRepository
        name: open-telemetry
        namespace: default
  releaseName: otel-collector
  targetNamespace: default
  values:
    mode: deployment
```

Visit [Flux API reference for v2](https://fluxcd.io/flux/components/helm/api/v2/#helm.toolkit.fluxcd.io/v2.HelmRelease) for detail spec information.

#### Kubernetes Health

Kubernetes already implements [Pod lifecycle](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#pod-phase), and [Liveness](https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/), [Readiness and Startup Probes](https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/) mechanism which is a standard for all containers running on the cluster and can be used as a generic interface to understand the health of a sub agent.

Any agent deployed in Kubernetes can be composed of several components and those components deployed under different Pods and Replication Controllers. For instance, nri-kubernetes contains 1 DaemonSet and 2 Deployments.

That's why the Agent Control leverages the Kubernetes Rust SDK to retrieve the health of standard replication controllers (Deployment, DaemonSet, StatefulSet) of the Agent at a configurable interval.

As a result, the health section for a Kubernetes deployment is as simple as this:

```yaml
deployment:
  health:
    interval: 30s
  objects:
    ...
```

Users can currently only configure the interval of those periodic health check, within the Agent Type. However, in the future, we could offer the end users the possibility of selecting what information should be retrieved.

#### Kubernetes Version

Version is checked periodically by querying the corresponding k8s object in the cluster. The Agent Type allows setting up the version
check interval and initial delay:

```yaml
deployment:
  version:
    interval: 120s # Defaults to 60s..
    initial_delay: 10s # Defaults to 30s.
```

## Development Custom agent types

Agent Control ships a set of embedded agent types (see
[`agent-control/agent-type-registry/newrelic/`](../agent-control/agent-type-registry/newrelic)). You can add your own —
or override an embedded one — by placing the definition YAML in the *dynamic agent types* directory. Custom definitions
take precedence over any other definition with the same id (namespace + name + version).

The current layout expects **a directory** at `/etc/newrelic-agent-control/dynamic-agent-types/` containing one file per
agent type.

Before dropping a definition into that directory, you can check its schema (required fields, field types, and format
constraints) without running Agent Control at all:

```sh
newrelic-agent-control-cli agent-type validate --file my-custom-agent-type.yaml
```

### On-host

This guideline shows how to build a custom agent type and integrate it with the agent control on-host. The [telegraf agent](https://www.influxdata.com/time-series-platform/telegraf/) is used as a reference.

1. Create a file with the agent type definition

    ```yaml
    # namespace: newrelic, external, other
    namespace: external
    # name: reverse FQDN that uniquely identifies the agent type
    name: com.influxdata.telegraf
    # version: semver scheme
    version: 0.0.1
    # protocol_version: quoted MAJOR.MINOR of the agent-type schema language
    protocol_version: "1.0"
    # platform: host or kubernetes
    platform: host
    # operating_system: required when platform is host. linux or windows
    operating_system: linux

    # variables:
    #   my_var_1:
    #     description: "Variable description here"
    #     type: string
    #     required: false
    #     default: "default value"

    variables:
      config_file:
        description: "Telegraf config file path"
        type: string
        required: false
        default: "/path/to/telegraf.conf"
      backoff_delay:
        description: "seconds until next retry if agent fails to start"
        type: string
        required: false
        default: 20s

    deployment:
      executables:
        - id: telegraf
          path: /usr/bin/telegraf
          args:
            - --config
            - ${nr-var:config_file}
          env: ""
          restart_policy:
            backoff_strategy:
              type: fixed
              backoff_delay: ${nr-var:backoff_delay}
    ```

2. Copy the agent type definition to the folder `/etc/newrelic-agent-control/dynamic-agent-types`
3. Use the new type in the `agents` config for the agent control:

    ```yaml
    # fleet_control:
    # ...
    
    agents:
      my-telegraf-collector:
        agent_type: "external/com.influxdata.telegraf:0.0.1"
    ```

4. If any `required` variable has been defined in the type or any default value for variables needs to be customized, then define a `values.yaml` in `/etc/newrelic-agent-control/fleet/agents.d/my-telegraf/values.yaml`:

    ```yaml
    config_file: /custom/path/to/file
    backoff_delay: 30s
    ```

5. Restart Agent Control.

### Kubernetes

In-cluster, the same `/etc/newrelic-agent-control/dynamic-agent-types/` directory is populated by mounting a ConfigMap.
Each ConfigMap key becomes a file inside the directory, so a single ConfigMap can carry multiple agent types.

1. Create the ConfigMap in the namespace where Agent Control runs. The `--from-file=<key>=<path>` form names each
   entry; that `<key>` is the file name that will appear under the mount:

   ```sh
   kubectl create configmap dynamic-agent \
     --from-file=dynamic-agent-type=./agent-control/agent-type-registry/newrelic/kubernetes-com.newrelic.infrastructure-0.1.0.yaml \
     -n newrelic-agent-control
   ```

   For multiple types, append more `--from-file=<key>=<path>` — one per file.

2. Mount that ConfigMap on the Agent Control pod through the deployment chart values, matching the layout used in the
   dynamic-agent-type k8s e2e ([`test/k8s-e2e/dynamic/ac-values-dynamic.yml`](../test/k8s-e2e/dynamic/ac-values-dynamic.yml)):

   ```yaml
   agentControlDeployment:
     chartValues:
       # [...]
       extraVolumeMounts:
         - name: dynamic
           mountPath: /etc/newrelic-agent-control/dynamic-agent-types
           readOnly: true
       extraVolumes:
         - name: dynamic
           configMap:
             name: dynamic-agent
   ```

3. Reference the type by its id in the AC config and supply variable values through `agentsConfig`. Each key under an
   agent's `agentsConfig` entry maps to a variable declared by the type:

   ```yaml
   agentControlDeployment:
     chartValues:
       config:
         agents:
           infra:
             agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
    # [...]
   ```

   To pick up a fresh ConfigMap, `helm upgrade` (or restart the AC pod).
