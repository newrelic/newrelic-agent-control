# E2e action based test

This test are executed by
the [newrelic-integration-e2e](https://github.com/newrelic/newrelic-integration-e2e-action/tree/main) spawning the Tilt
local environment with a custom configuration for the test to execute a Sub-Agent and then check that metrics sent by
this Sub-Agent have reached NR.

# Run locally

To be able to run the tests these dependencies should be installed
```bash
brew install coreutils
brew install jq
```

# Run locally

The test leverages the Tilt environment so all requirements to launch Tilt must be followed.
Notice that the local execution expect to use Testing account.

The `ac-values-*.yml` files take agent and agent-type versions from `${<AGENT>_VERSION}` and `${<AGENT>_AGENT_TYPE_VERSION}`
placeholders, which CI fills in with the versions of Instrumentation Metadata (`.github/actions/agent-versions/load`).
To run locally, export the `<AGENT>_VERSION` and `<AGENT>_AGENT_TYPE_VERSION` variables used by the values files you run
(for example `NRINFRA_VERSION` and `NRINFRA_AGENT_TYPE_VERSION`; the names are the uppercased Instrumentation Metadata agent
types) and run `test/k8s-e2e/render-agent-versions.sh`. It fails if a placeholder has no value and edits the files in
place, so do not commit the result.

```bash
export ACCOUNT_ID=<NewRelic Staging testing account number>
export API_REST_KEY=<NewRelic Staging testing api rest key>
export LICENSE_KEY=<NewRelic Staging testing ingest key>
export SPEC_PATH=<E2e spec path - Example "e2e-apm.yml">
minikube start --driver='docker'
make test/k8s-e2e
```

# Feature branch workaround

A change on the SA could be not compatible with the latest released chart, so there is a workaround to execute the tests
installing the charts from a feature branch in `https://github.com/newrelic/helm-charts`.
In order to use the branch for the tests modify `feature_branch = '<feature-branch>'`
configurations in the Tiltfile.
