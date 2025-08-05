# E2E action-based test for on-host

This test are executed by the
[newrelic-integration-e2e](https://github.com/newrelic/newrelic-integration-e2e-action/tree/main)
spawning lightweight VMs with a custom configuration for the test to execute a Sub-Agent and then
check that metrics sent by this Sub-Agent have reached NR.

It also specifically tests the migration scenario from existing Infrastructure Agent setups.

## Prerequisites

To be able to run the tests these dependencies should be installed

```bash
brew install lima # Linux VMs
brew install lima-additional-guestagents # For testing in non-native architectures such as x86_64
brew install just # Command runner
```

## Run locally

The test leverages the Tilt environment so all requirements to launch Tilt must be followed.
Notice that the local execution expect to use Testing account.

```bash
export ACCOUNT_ID=<NewRelic Staging testing account number>
export API_REST_KEY=<NewRelic Staging testing api rest key>
export LICENSE_KEY=<NewRelic Staging testing ingest key>
export SPEC_PATH=<E2e spec path - Example "e2e-apm.yml">
minikube start --driver='docker'
make test/k8s-e2e
```
