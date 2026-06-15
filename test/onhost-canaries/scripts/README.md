# Fleet Deployment Script

Script to create and trigger Fleet Control deployments for on-host canaries.

## Overview

The `fleet_deployment.sh` script creates a Fleet Control deployment that defines the desired state of agents in a fleet and triggers the rollout through the ring deployment policy.

### Two-Step Process

1. **fleetControlCreateFleetDeployment** — Creates the deployment definition
2. **fleetControlDeploy** — Pushes it through the ring policy to the fleet

## Prerequisites

### Required Tools

- `bash` (tested with bash 5.x)
- `curl`
- `jq`

### Required Environment Variables

Set these environment variables before running the script:

```bash
export NEW_RELIC_API_KEY="<your-api-key>"      # NerdGraph User API key
export FLEET_ID="<fleet-entity-guid>"          # Fleet entity GUID
export SCOPE_ORG_ID="<organization-guid>"      # Organization GUID for deployment scope
export ENVIRONMENT="staging"                   # "staging" or "production"
```

## Usage

### Purpose
The idea behind the script is to create deployments with different configs that set the agents from the fleet in a 
specific state to create noise in the canaries and test all scenarios.
We need to create different configs that will be passed to this script to cover scenarios like:
- Adding one agent.
- Removing an agent.
- Removing all agents.


### Basic Syntax

```bash
./fleet_deployment.sh <agentType>:<version>:<configVersionId> [<agentType>:<version>:<configVersionId> ...]
```

Each agent spec has three parts separated by colons:
- **`agentType`**: The agent type identifier (e.g., `NRInfra`, `NRDOT`, `com.newrelic.prometheus`)
- **`version`**: The agent version to deploy (e.g., `1.76.1`)
- **`configVersionId`**: The AGENT_CONFIGURATION_VERSION entity GUID (base64-encoded)
  **IMPORTANT:** You need an `AGENT_CONFIGURATION_VERSION` entity ID, not an `AGENT_CONFIGURATION` ID. 
  This configuration ID is from a configuration existing in Fleet.

### Supported Agent Types

Common agent type identifiers:

| Agent Type Identifier | Description |
|----------------------|-------------|
| `NRInfra` | New Relic Infrastructure Agent |
| `NRDOT` | New Relic Distro for OpenTelemetry |

### Examples

#### Single Agent Deployment

Deploy Infrastructure Agent version 1.76.1 with a specific configuration:

```bash
export NEW_RELIC_API_KEY="NRAK-XXXX"
export FLEET_ID="MTIyMTMwNjh8TkdFUHxGTEVFVHwwMTlhZTNiNS01Yjg5LTdkNjYtYWU0MC1lNmZkOTY2ZDFhMDA"
export SCOPE_ORG_ID="9d789cca-f661-458d-be06-882d1e6e409d"
export ENVIRONMENT="staging"

./fleet_deployment.sh "NRInfra:1.76.1:MTIyMTMwNjh8TkdFUHxBR0VOVF9DT05GSUdVUkFUSU9OX1ZFUlNJT058MDE5YzdhYWEtNmM4My03NWFhLWIzYmEtOTE0MjIzZDU0Mjk1"
```

#### Multiple Agent Deployment

Deploy both Infrastructure Agent and Otel:

```bash
./fleet_deployment.sh \
  "NRInfra:1.76.1:MTIyMTMwNjh8TkdFUHxBR0VOVF9DT05GSUdVUkFUSU9OX1ZFUlNJT058MDE5YzdhYWEtNmM4My03NWFhLWIzYmEtOTE0MjIzZDU0Mjk1" \
  "NRDOT:1.3.0:NjQyNTg2NXxOR0VQfEFHRU5UX0NPTkZJR1VSQVRJT05fVkVSU0lPTnwwMTllODc0Ni1mN2Q4LTdkOWUtYWU1Yy1jNjM3ZTUxMjRmYjc"
```

## Output

The script outputs:
- Deployment creation progress
- Deployment ID (entity GUID)
- Trigger confirmation

Example output:

```
[2026-06-10 11:34:52] =======================================
[2026-06-10 11:34:52] Fleet Deployment Script
[2026-06-10 11:34:52] =======================================
[2026-06-10 11:34:52] Environment  : staging
[2026-06-10 11:34:52] Fleet ID     : MTIyMTMwNjh8TkdFUHxGTEVFVHwwMTlhZTNiNS01Yjg5LTdkNjYtYWU0MC1lNmZkOTY2ZDFhMDA
[2026-06-10 11:34:52] Scope Org ID : 9d789cca-f661-458d-be06-882d1e6e409d
[2026-06-10 11:34:52] Deployment   : canary-deployment-20260610-113452
[2026-06-10 11:34:52] Agents       : NRInfra:1.76.1:MTIyMTMwNjh8TkdFUHxBR0VOVF9DT05GSUdVUkFUSU9OX1ZFUlNJT058MDE5YzdhYWEtNmM4My03NWFhLWIzYmEtOTE0MjIzZDU0Mjk1
[2026-06-10 11:34:52] =======================================
[2026-06-10 11:34:54] Deployment created — ID: MTIyMTMwNjh8TkdFUHxGTEVFVF9ERVBMT1lNRU5UfDAxOWViMGUyLTdmYzAtN2Y3ZC04NWE4LWYyNTFmMmRjZGRmNA
[2026-06-10 11:34:55] Deployment triggered — confirmed ID: MTIyMTMwNjh8TkdFUHxGTEVFVF9ERVBMT1lNRU5UfDAxOWViMGUyLTdmYzAtN2Y3ZC04NWE4LWYyNTFmMmRjZGRmNA
[2026-06-10 11:34:55] =======================================
[2026-06-10 11:34:55] Done.
[2026-06-10 11:34:55] DEPLOYMENT_ID=MTIyMTMwNjh8TkdFUHxGTEVFVF9ERVBMT1lNRU5UfDAxOWViMGUyLTdmYzAtN2Y3ZC04NWE4LWYyNTFmMmRjZGRmNA
[2026-06-10 11:34:55] =======================================
```

## Integration with CI/CD
We can call this script from a Github action that is ran on a scheduled range and depending on the execution adds on agent or other.
It's important to note the need to use linux-4-core-nr-control runner to access the Staging APIs. 



