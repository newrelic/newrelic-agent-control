# Agent Type Overview

Agent Type Definition is a YAML file that defines an agent's configuration and behavior. It consists of three main sections: metadata, deployment, and variables.

## Metadata

The metadata section contains information about the agent type, such as its name and version. This section also includes the agent's namespace, which is used to organize related agents and their configurations.

```yaml
namespace: newrelic/agent-types
name: io.opentelemetry.collector
version: 0.0.1
```

## Variables

The variables section allows developers to define variables that can be set by the final user. These variables can be used to customize the agent's or configuration.
```yaml
variables:
  on_host:
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
    backoff_delay:
      description: "seconds until next retry if agent fails to start"
      type: string
      required: false
      default: 20s
    enable_file_logging:
      description: "enable logging the on host executables' logs to files"
      type: bool
      required: false
      default: false
  k8s:
    chart_values:
      description: "Newrelic otel collector chart values"
      type: yaml
      required: true
    chart_version:
      description: "Newrelic otel collector chart version"
      type: string
      required: true
      default: "0.78.3"
```

All variables have a few common attributes:

* `description`: A brief description of the variable.This is useful for documentation purposes and can help others understand the purpose of the variable.
* `type`: The data type of the variable. We support several data types, including `string`, `file`, `bool`, `yaml`, and more. 
* `default`: The default value for the variable if no value is provided.. 
* `required`: Whether the variable is mandatory to be provided or not.

And file type variables contain one additional attribute:
* `file_path`: The path where the file is located.

In terms of available variable types, we currently support the following that can be found [here](variable/kind.rs#L14):

* `string`: A string value, such "Hello, world!"
* `number`: An integer value, such as 42
* `boolean`: A boolean value, which can be either *true* or *false*
* `file`: It represents a file in the filesystem.
* `map[string]string`: A dictionary of key-value pairs, associating keys(string) and values(string).
* `map[string]file`: A dictionary of key-value pairs, associating keys(string) and values(file).
* `yaml`: The YAML type variable is used to handle multi-line strings that will be parsed as YAML such as Helm Charts values.


## Deployment

The deployment section defines how the agent should be executed or health checked. It includes details such as the executable path, command-line arguments, environment variables, and restart policy.

Note you can reference the variables defined in the previous section.

```yaml
deployment:
  on_host:
    enable_file_logging: ${nr-var:enable_file_logging}
    executables:
      - path: /usr/bin/newrelic-infra
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

By defining these three sections, developers can create a customizable and flexible agent type that can be used in various environments. 