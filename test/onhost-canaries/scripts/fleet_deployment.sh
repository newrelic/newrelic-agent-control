#!/usr/bin/env bash
#
# fleet_deployment.sh
#
# Generic script to create Fleet Control deployments for on-host canaries.
# Defines the DESIRED STATE of agents in a fleet and triggers the rollout.
#
# TWO-STEP PROCESS:
#   1. fleetControlCreateFleetDeployment  →  creates the deployment definition
#   2. fleetControlDeploy                 →  pushes it through the ring policy
#
# ---------------------------------------------------------------------------
# USAGE
# ---------------------------------------------------------------------------
#   ./fleet_deployment.sh <agent-spec> [<agent-spec> ...]
#
#   Each <agent-spec> has the form:
#     <agentType>:<version>:<configVersionId>
#
#   Examples:
#     # Single agent
#     ./fleet_deployment.sh "NRInfra:1.76.1:NjQyNTg2NX..."
#
#     # Two agents (Fleet Control reconciles additions/removals automatically)
#     ./fleet_deployment.sh "NRInfra:1.76.1:NjQyNTg2NX..." "com.newrelic.prometheus:1.3.0:abc123..."
#
# ---------------------------------------------------------------------------
# REQUIRED environment variables
# ---------------------------------------------------------------------------
#   NEW_RELIC_API_KEY   NerdGraph User API key
#   FLEET_ID            Fleet entity GUID
#   SCOPE_ORG_ID        Organization GUID used as deployment scope
#   ENVIRONMENT         "staging" or "production"

set -euo pipefail

# ---------------------------------------------------------------------------
# Validate required environment variables
# ---------------------------------------------------------------------------
REQUIRED_VARS=(NEW_RELIC_API_KEY FLEET_ID SCOPE_ORG_ID ENVIRONMENT)
for var in "${REQUIRED_VARS[@]}"; do
  if [[ -z "${!var:-}" ]]; then
    echo "ERROR: Required environment variable '${var}' is not set." >&2
    exit 1
  fi
done

if [[ "${ENVIRONMENT}" != "staging" && "${ENVIRONMENT}" != "production" ]]; then
  echo "ERROR: ENVIRONMENT must be 'staging' or 'production', got '${ENVIRONMENT}'" >&2
  exit 1
fi

if [[ $# -eq 0 ]]; then
  echo "ERROR: At least one agent spec is required." >&2
  echo "Usage: $0 <agentType>:<version>:<configVersionId> [...]" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
DEPLOYMENT_NAME="canary-deployment-${TIMESTAMP}"
# When a new fleet is created both rings are always created, all hosts added to a fleet are added to default unless
# they are moved to canary; to warrant all hosts are always deployed, we always deploy both rings, even if in the vast
# majority of cases only default exists.
RINGS_TO_DEPLOY='["canary", "default"]'

if [[ "${ENVIRONMENT}" == "production" ]]; then
  NERDGRAPH_URL="https://api.newrelic.com/graphql"
else
  NERDGRAPH_URL="https://staging-api.newrelic.com/graphql"
fi

# ---------------------------------------------------------------------------
# Logging
# ---------------------------------------------------------------------------
log() {
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >&2
}

# ---------------------------------------------------------------------------
# NerdGraph helper
# ---------------------------------------------------------------------------
call_nerdgraph() {
  local mutation="$1"
  local payload
  payload=$(jq -n --arg query "${mutation}" '{"query": $query}')

  local response
  response=$(
    curl --fail --silent --show-error \
      --max-time 30 \
      -X POST "${NERDGRAPH_URL}" \
      -H "Content-Type: application/json" \
      -H "API-Key: ${NEW_RELIC_API_KEY}" \
      -d "${payload}"
  )

  if echo "${response}" | jq -e '.errors' > /dev/null 2>&1; then
    log "ERROR: NerdGraph returned errors:" >&2
    echo "${response}" | jq '.errors' >&2
    exit 1
  fi

  echo "${response}"
}

# ---------------------------------------------------------------------------
# Build a single agent GraphQL input fragment from a spec string.
#
# Spec format: <agentType>:<version>:<configVersionId>
# ---------------------------------------------------------------------------
parse_agent_spec() {
  local spec="$1"
  local agent_type version config_id

  IFS=':' read -r agent_type version config_id <<< "${spec}"

  if [[ -z "${agent_type}" || -z "${version}" || -z "${config_id:-}" ]]; then
    log "ERROR: Invalid agent spec '${spec}'. Expected <agentType>:<version>:<configVersionId>" >&2
    exit 1
  fi

  printf '{configurationVersionList: {id: "%s"}, agentType: "%s", version: "%s"}' \
    "${config_id}" "${agent_type}" "${version}"
}

# ---------------------------------------------------------------------------
# Build the agents GraphQL input array from one or more pre-built agent
# fragments. `paste -sd ','` uses comma as a separator between lines, so no
# trailing comma is produced.
#   [{agentType: "..."}]                        # single
#   [{agentType: "..."}, {agentType: "..."}]    # multiple
# ---------------------------------------------------------------------------
build_agents_input() {
  local joined
  joined=$(printf '%s\n' "$@" | paste -sd ',')
  echo "[${joined}]"
}

# ---------------------------------------------------------------------------
# Step 1: Create a fleet deployment definition
# ---------------------------------------------------------------------------
create_fleet_deployment() {
  local agents_input="$1"

  log "Creating deployment '${DEPLOYMENT_NAME}' on fleet '${FLEET_ID}'..."
  log "Agents input: ${agents_input}"

  local mutation
  mutation="mutation {
  fleetControlCreateFleetDeployment(
    fleetDeployment: {
      agents: ${agents_input}
      fleetId: \"${FLEET_ID}\"
      scope: {type: ORGANIZATION, id: \"${SCOPE_ORG_ID}\"}
      name: \"${DEPLOYMENT_NAME}\"
    }
  ) {
    entity {
      name
      id
      description
    }
  }
}"

  local response
  response=$(call_nerdgraph "${mutation}")

  local deployment_id
  deployment_id=$(echo "${response}" | jq -r '.data.fleetControlCreateFleetDeployment.entity.id')

  if [[ -z "${deployment_id}" || "${deployment_id}" == "null" ]]; then
    log "ERROR: Failed to extract deployment ID from response:" >&2
    echo "${response}" | jq '.' >&2
    exit 1
  fi

  log "Deployment created — ID: ${deployment_id}"
  echo "${deployment_id}"
}

# ---------------------------------------------------------------------------
# Step 2: Trigger the deployment through the ring policy
# ---------------------------------------------------------------------------
trigger_deployment() {
  local deployment_id="$1"

  log "Triggering deployment '${deployment_id}'..."

  local mutation
  mutation="mutation {
  fleetControlDeploy(
    id: \"${deployment_id}\"
    policy: {ringDeploymentPolicy: {ringsToDeploy: ${RINGS_TO_DEPLOY}}}
  ) {
    id
  }
}"

  local response
  response=$(call_nerdgraph "${mutation}")

  local triggered_id
  triggered_id=$(echo "${response}" | jq -r '.data.fleetControlDeploy.id')

  if [[ -z "${triggered_id}" || "${triggered_id}" == "null" ]]; then
    log "ERROR: Deployment trigger failed. Response:" >&2
    echo "${response}" | jq '.' >&2
    exit 1
  fi

  log "Deployment triggered — confirmed ID: ${triggered_id}"
  echo "${triggered_id}"
}

main() {
  log "======================================="
  log "Fleet Deployment Script"
  log "======================================="
  log "Environment  : ${ENVIRONMENT}"
  log "Fleet ID     : ${FLEET_ID}"
  log "Scope Org ID : ${SCOPE_ORG_ID}"
  log "Deployment   : ${DEPLOYMENT_NAME}"
  log "Agents       : $*"
  log "======================================="

  # Parse each agent spec into a GraphQL input fragment
  local agent_fragments=()
  for spec in "$@"; do
    agent_fragments+=("$(parse_agent_spec "${spec}")")
  done

  local agents_input
  agents_input=$(build_agents_input "${agent_fragments[@]}")

  # Step 1 — Create the deployment definition
  local deployment_id
  deployment_id=$(create_fleet_deployment "${agents_input}")

  # Step 2 — Trigger the deployment
  trigger_deployment "${deployment_id}"

  log "======================================="
  log "Done."
  log "DEPLOYMENT_ID=${deployment_id}"
  log "======================================="

  # Emit a machine-readable line for CI to capture
  echo "DEPLOYMENT_ID=${deployment_id}"
}

main "$@"
