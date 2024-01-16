# e2e action based test

This test are executed by the [newrelic-integration-e2e](https://github.com/newrelic/newrelic-integration-e2e-action/tree/main) spawning the Tilt local environment with a custom configuration for the test to execute a Sub-Agent and then check that metrics sent by this Sub-Agent have reached NR.

# Run locally
The test leverages the Tilt environment so all requirements to launch Tilt must be followed.

```bash
export ACCOUNT_ID=<NewRelic staging account number>
export API_REST_KEY=<NewRelic staging api rest key>
export LICENSE_KEY=<NewRelic staging ingest key>
ctlptl create registry ctlptl-registry --port=5005 && ctlptl create cluster minikube --registry=ctlptl-registry
make test/k8s-e2e
```

# Feature branch workaround 

A change on the SA could be not compatible with the latest released chart, so there is a workaround to execute the tests installing the charts from a feature branch in `https://github.com/newrelic/helm-charts`.
In order to use the branch for the tests modify the following lines in the Tiltfile:
```python
force_workaround = True
feature_branch = '<branch-name>'
```
