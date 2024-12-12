# Agent Type Overview

Agent Type Definition is a YAML file that defines an agent's configuration and behavior. It consists of three main sections: metadata, deployment, and variables.

By defining these three sections, developers can create a customizable and flexible agent type that can be used in various environments. 

## Metadata

The metadata section contains information about the agent type, such as its name and version. This section also includes the agent's namespace, which is used to organize related agents and their configurations.

```yaml
namespace: newrelic
name: io.opentelemetry.collector
version: 0.0.1
```

The metadata fields can't be empty:
* The name and namespace should:
  - Start by an alphabetical character.
  - Only encompass `alphanumeric characters`, `.`, `_` or `-`.
  - Be in lowercase.
* The version field should adhere to [semantic versioning](https://semver.org/).

## Variables

The variables section allows developers to define variables that end users can set. These variables can adjust the agent's or system's configuration.

```yaml
variables:
  common:
    config_agent:
      description: "Newrelic infra configuration"
      type: file
      required: false
      default: ""
      file_path: "newrelic-infra.yml"
    config_integrations:
      description: "map of YAML configs for the OHIs"
      type: map[string]file
      required: false
      default: {}
      file_path: "integrations.d"
  on_host:
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
  k8s:
     ...
```

Variables can be classified based on their applicable environments:
* `on_host`: Refers to variables utilized in on host environments.
* `k8s`: Applies to variables used within Kubernetes clusters.
* `common`: For variables that are environment-agnostic.

Although a variable name can be concurrently specified under both the `k8s` and `on_host` sections, it's necessary to note an exception. If a variable is already defined in the `common` section, it cannot be duplicated under any other section. In other words, any variables named in the `common` section must be unique and not repeated in either the `k8s` or the `on_host` sections.

Nested variable names are supported. For instance:

```yaml
common:
  log:
    level:
      description: "Log level with only info and error"
      type: string
      required: false
      default: info
      variants: ["info", "error"]
```

All variables have a few common attributes:

* `description`: A brief description of the variable.This is useful for documentation purposes and can help others understand the purpose of the variable.
* `type`: The data type of the variable. We support several data types, including `string`, `file`, `bool`, `yaml`, and more. 
* `variants`: Represents a defined list of acceptable values for the variable. Only values present in the variants list are considered valid.
* `default`: The default value for the variable if no value is provided.
* `required`: Whether the variable is mandatory to be provided or not.

Moreover, file type variables possess an additional attribute:
* `file_path`: Indicates where the file(s) is located. 

In terms of variable types, we currently support the following types listed [here](variable/kind.rs#L14):

* `string`: A string value, such as "Hello, world!"
* `number`: An numeric value, such as 42 or 0.25
* `boolean`: A boolean value, which can be either *true* or *false*
* `file`: It represents a file in the filesystem.
* `map[string]string`: A dictionary of key-value pairs, associating keys(string) and values(string).
* `map[string]file`: A dictionary of key-value pairs, associating keys(string) and values(file). Files are stored in the specified folder.
* `yaml`: The YAML type variable is used to handle multi-line strings that will be parsed as YAML such as Helm Charts values.

Note that `file` and `map[string]file` variable types can be used just for `on_host` environments. Files are not supported in Kubernetes.

## Deployment

The deployment section indicates how the agent should be executed and its health should be checked.

Note you can reference the variables defined in the `variables` section using `${nr-var:variable_name}`. And this is valid for nested variables as well: following the example above, you would be able to use `${nr-var:log.info}`.

### OnHost Deployment

For on-host deployment, use the following format:

```yaml
deployment:
  on_host:
    enable_file_logging: ${nr-var:enable_file_logging}
    executable:
      path: /usr/bin/newrelic-infra
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

In this section:

* `enable_file_logging`: This setting turns on logging for the agent supervisor
* `executables`: This outlines the list of binaries the agent supervisor runs. Developers can define:
  * `path`: The location of the binary required.
  * `args`: The command-line arguments needed by the binary.
  * `env`: Specifies the required environment variables.
* `restart_policy`: The guidelines for if or when the process should be restarted.
* `health`: The measures used to check the health status of the agent.

These diverse options offer extensive customization for your agent's deployment.

#### Restart Policy

`restart_policy` provides a set of instructions on how and when the agent process should be restarted. It's crucial for maintaining the agent's availability and reliability, particularly in case of unexpected failures or problems. 

In the `backoff_strategy` we have:

* `type`: This field can take several forms - _none_, _fixed_, _linear_, or _exponential_. It determines the delay timing strategy between retries.
  * _none_: Means no delay between retries.
  * _fixed_: Constant delay interval between retries. This is the default type.
  * _linear_: Delay interval increases linearly after each retry.
  * _exponential_: Delay interval doubles after each retry.
* `backoff_delay`: It defines the duration between retries when a restart is needed. This delay protects against aggressive restarts. Default is _2s_.
* `max_retries`: This integer value defines the maximum number of retry attempts before exiting the retry mechanism and accepting the failure. Default is _0_.
* `last_retry_interal`:  This is used to store the duration of the last delay. It can especially be relevant in case of _linear_ or _exponential_ backoff strategies where each retry level has a different delay value. Default is _600_.

#### OnHost Health

The `health` section in the deployment configuration is where you can specify how to monitor the health status of the agent. This is critical for maintaining the reliability of your agent and ensuring that it's functioning correctly. Here's how you can define it in the `executables` block:

```yaml
health:
    interval: 5s
    timeout: 5s
    http:
        path: "/v1/status"
        port: 8003
        healthy_codes: [200,203,203,204]                 
```

In this configuration:

* `interval`: This parameter specifies the frequency at which health checks should be performed. 
* `timeout`: This is the maximum time the agent should wait for a health check response. 
* `http`: This section is for when agents expose their status through an HTTP endpoint. If this method is used, the `path` and `port` should be specified.
  * `path`: This is the API endpoint for the health check. Typically it's a URI where the agent returns its current health status.
  * `port`: This is the port on which the agent's health check endpoint is listening.
  * `healthy_codes`: This is a list of the HTTP codes the SA will consider as valid ones.

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

### Kubernetes Deployment

The Agent Control leverages [Flux](https://fluxcd.io/) to act as an operator running Helm commands (install, upgrade, delete) as needed based on the provided configurations.

Then, for a Kubernetes deployment, we use the following format:

```yaml
deployment:
  k8s:
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
              reconcileStrategy: ChartVersion
              sourceRef:
                kind: HelmRepository
                name: ${nr-sub:agent_id}
              interval: 3m
          install:
            # Wait are disabled to avoid blocking the modifications/deletions of this CR while in reconciling state.
            disableWait: true
            disableWaitForJobs: true
            remediation:
              retries: 3
            replace: true
          upgrade:
            disableWait: true
            disableWaitForJobs: true
            cleanupOnFail: true
            force: true
            remediation:
              retries: 3
              strategy: rollback
          rollback:
            disableWait: true
            disableWaitForJobs: true
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

That's why the health section for a Kubernetes deployment is as simple as this:

```yaml
deployment:
  k8s:
    health:
      interval: 30s
    objects:
      ...
```

Users can currently only configure the interval of those periodic health check, within the Agent Type. However, in the future, we could offer the end users the possibility of selecting what information should be retrieved.

## Development

This guideline shows how to build a custom agent type and integrate it with the agent control on-host. The [telegraf agent](https://www.influxdata.com/time-series-platform/telegraf/) is used as a reference.

1. Create a `dynamic-agent-type.yaml` file with the agent type definition

```yaml
# namespace: newrelic, external, other
namespace: external
# name: reverse FQDN that uniquely identifies the agent type
name: com.influxdata.telegraf
# version: semver scheme
version: 0.0.1

# variables:
#   common | on_host | k8s:
#     my_var_1:
#       description: "Variable description here"
#       type: file | string
#       required: true | false
#       default: "default value"
#       file_path: "persistence path"

variables:
  on_host:
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
  on_host:
    executable:
      path: /usr/bin/telegraf
      args: "--config ${nr-var:config_file}"
      env: ""
      restart_policy:
        backoff_strategy:
          type: fixed
          backoff_delay: ${nr-var:backoff_delay}
```

2. Copy the agent type definition to `/etc/newrelic-agent-control/dynamic-agent-type.yaml`

    ⚠︎ This is a temporal path, expect a configurable path to load custom agent types in the future.

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

5. Restart the agent control.
