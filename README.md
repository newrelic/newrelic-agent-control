# supervisor

WIP universal supervisor, powered by OpAMP.

## Sample default static config (/tmp/static.yaml)

```yaml
opamp:
  endpoint: https://opamp.service.newrelic.com/v1/opamp
  headers:
    api-key: API_KEY_HERE

 agents:
  nr_infra_agent:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.1"
  nr_otel_collector:
    agent_type: "newrelic/io.opentelemetry.collector:0.0.1"
```

## Development

### Running New Relic Super Agent Locally in Kubernetes

We use [Minikube](https://minikube.sigs.k8s.io/docs/) and [Tilt](https://tilt.dev/) to launch a local cluster and deploy the Super Agent [charts](https://github.com/newrelic/helm-charts/tree/master/charts).

#### Prerequisites:
- Ensure you have kubectl installed and properly configured.
- Install Minikube for local Kubernetes cluster emulation.
- Ensure you have Tilt installed for managing local development environments.

#### Steps
```
make tilt-up
```
This will spin up the Super Agent within your local Minikube Kubernetes cluster. You can view the Super Agent logs and services using Tilt's web interface or using standard kubectl commands.
