# Support existing Flux

Agent Control supports using an existing Flux installation. In other words, we can install Agent Control in a k8s cluster where Flux is already present. However, we **DON’T HAVE** extensive and complete support for that. There are some limitations to that feature.

## Requirements

> [!WARNING]  
> Ensure the cluster complies with the following requirements. Otherwise, we are not responsible for any malfunction of the product or any potential breakage of the cluster.

* Flux version 2
    * Helm Controller component
        * HelmRelease CRD from helm.toolkit.fluxcd.io/v2
    * Source Controller component
        * HelmRepository CRD from source.toolkit.fluxcd.io/v1
* ClusterRole for Flux with sufficient permissions
* Flux is configured to watch resources in the namespace where Agent Control will be installed

## How do we configure AC to work with an already existing Flux?

Disable agent-control-cd. This is very straightforward.

```yaml
agentControlCd:
  enabled: false
```

The final config would look something like the following:

```yaml
global:
  cluster: "xxx"
  licenseKey: "xxx"

agentControlCd:
  enabled: false

agentControlDeployment:
  chartValues:
    subAgentsNamespace: "newrelic"
    config:
      fleet_control:
        fleet_id: "xxx"
    systemIdentity:
      organizationId: "xxx"
      parentIdentity:
        clientId: "xxx"
        clientSecret: "xxx"
```

## What's the minimum set of permissions required for the Cluster Role?

This depends on the agents that we plan to install with Agent Control. The permissions is the sum of the permissions needed by:

* `HelmController`
* `SourceController`
* Agent Control
* Every agent we want to install

Alternatively, we can use `cluster-admin`. This grants root privileges and it's [used by default in the Flux chart](https://github.com/fluxcd-community/helm-charts/tree/main/charts/flux2).

## How does Flux watched namespaces influence Agent Control?

Agent Control must be installed on a namespace watched by Flux. Otherwise, agents won't be installed. Now, we can find ourselves in two situations.

First, Flux is configured to watch every namespace (`--watch-all-namespaces` is true). In that case, we can install Agent Control in any namespace and it will work out of the box.

Second, Flux is configured to only watch the runtime namespace (`--watch-all-namespaces` is false). Then, we need to install Agent Control in the same namespace where Flux was installed.
