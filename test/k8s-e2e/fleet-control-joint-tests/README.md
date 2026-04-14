# Fleet Control Joint Tests

This directory contains configuration for Fleet Control E2E tests that run against K8s deployments.

## Why No e2e-*.yml Spec File?

Unlike other K8s E2E test scenarios, Fleet Control tests **do not use** the standard `newrelic-integration-e2e-action` framework because:

1. **Different test mechanism**: Fleet Control tests are driven by an external test controller API, not NRQL queries
2. **Different authentication**: Requires system identity JWT tokens, not just license keys
3. **Different flow**: Triggers external test suites and polls for completion via HTTP API
4. **Different timing**: Tests run asynchronously (5-15 minutes) outside the cluster

## Test Execution

Fleet Control tests are orchestrated directly by the GitHub Actions workflow:

- **Workflow**: [.github/workflows/fleet_control_k8s_e2e.yml](../../../.github/workflows/fleet_control_k8s_e2e.yml)
- **Test Driver**: [.github/workflows/scripts/fleet_control_api.sh](../../../.github/workflows/scripts/fleet_control_api.sh)
- **Values File**: [ac-values-fleet-control.yml](./ac-values-fleet-control.yml)

## Files in This Directory

- **ac-values-fleet-control.yml**: Helm values for deploying Agent Control with Fleet Control enabled
  - Configures system identity authentication
  - Sets fleet ID for the test
  - Enables debug logging
  - Deploys infra agent as a sub-agent

## Running Tests

### Via GitHub Actions
```bash
# Trigger manually with default fleet
gh workflow run fleet_control_k8s_e2e.yml

# Trigger with custom fleet ID
gh workflow run fleet_control_k8s_e2e.yml -f fleet_id="YOUR_FLEET_ID"
```

### Locally
```bash
# Set up environment
export NR_SYSTEM_IDENTITY_CLIENT_ID="your-client-id"
export NR_SYSTEM_IDENTITY_PRIVATE_KEY="your-private-key"
export FLEET_CONTROL_TOKEN="your-fc-token"
export FLEET_ID="your-fleet-id"

# Deploy to Minikube
kubectl create secret generic sys-identity \
  --from-literal=CLIENT_ID="${NR_SYSTEM_IDENTITY_CLIENT_ID}" \
  --from-literal=private_key="${NR_SYSTEM_IDENTITY_PRIVATE_KEY}"

CLUSTER="local-test" \
SA_CHART_VALUES_FILE="ac-values-fleet-control.yml" \
tilt ci

# Wait for AC to connect
sleep 60

# Run Fleet Control tests
bash ../../../.github/workflows/scripts/fleet_control_api.sh

# Cleanup
tilt down
```

## Fleet Configuration

The default fleet used for testing:
- **Name**: AC-FC E2E tests (k8s)
- **Account**: Test Automation (Staging)
- **Fleet ID**: `MTIyMTA0NzV8TkdFUHxGTEVFVHwwMTliZTAzZC03MDYwLTcxMDctOTUwYS04YTFiODc3YjJiN2Q`

## Related Documentation

- [Fleet Control Host Linux E2E](.github/workflows/fleet_control_host_linux_e2e.yml) - Similar tests for Linux hosts
- [Fleet Control Rust Implementation](../../e2e-runner/src/linux/scenarios/fleet_control.rs) - Linux test runner logic
