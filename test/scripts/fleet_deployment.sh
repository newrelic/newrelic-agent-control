#!/usr/bin/env bash
#
# fleet_deployment.sh
#
# Generic script to create Fleet Control deployments for on-host canaries.
# Defines the DESIRED STATE of agents in a fleet and triggers the rollout.
#
# Uses the New Relic CLI (`newrelic fleetcontrol deployment ...`) instead of
# hand-rolled NerdGraph mutations.
#
# TWO-STEP PROCESS:
#   1. newrelic fleetcontrol deployment create  →  creates the deployment definition
#   2. newrelic fleetcontrol deployment deploy  →  pushes it through the ring policy
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
#   NEW_RELIC_API_KEY   NerdGraph User API key (NRAK-...)
#   FLEET_ID            Fleet entity GUID
#   ENVIRONMENT         "staging" or "production"
#
# Note: the deployment scope (organization) is derived automatically by the CLI from the API key

set -euo pipefail

# ---------------------------------------------------------------------------
# Validate required environment variables
# ---------------------------------------------------------------------------
REQUIRED_VARS=(NEW_RELIC_API_KEY FLEET_ID ENVIRONMENT)
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
# When a new fleet is created both rings are always created; all hosts added to
# a fleet land in `default` unless moved to `canary`. To guarantee every host is
# always deployed we always deploy both rings, even though in the vast majority
# of cases only `default` exists. The CLI expects a comma-separated list.
RINGS_TO_DEPLOY="canary,default"

# The New Relic CLI reads NEW_RELIC_REGION from the environment. Map the
# script's ENVIRONMENT onto the CLI region: staging → Staging, production → US.
if [[ "${ENVIRONMENT}" == "production" ]]; then
  export NEW_RELIC_REGION="US"
else
  export NEW_RELIC_REGION="Staging"
fi

NEWRELIC_CLI_INSTALL_URL="https://download.newrelic.com/install/newrelic-cli/scripts/install.sh"

# ---------------------------------------------------------------------------
# Logging
# ---------------------------------------------------------------------------
log() {
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >&2
}

# ---------------------------------------------------------------------------
# Ensure the New Relic CLI is available; install it if missing (matches the
# manual bootstrap on fresh canary hosts).
# ---------------------------------------------------------------------------
ensure_newrelic_cli() {
  if command -v newrelic > /dev/null 2>&1; then
    return
  fi

  log "New Relic CLI not found — installing from ${NEWRELIC_CLI_INSTALL_URL}..."
  curl -Ls "${NEWRELIC_CLI_INSTALL_URL}" | bash

  if ! command -v newrelic > /dev/null 2>&1; then
    log "ERROR: New Relic CLI installation failed — 'newrelic' still not on PATH." >&2
    exit 1
  fi
}

# ---------------------------------------------------------------------------
# Step 1: Create a fleet deployment definition
# ---------------------------------------------------------------------------
create_fleet_deployment() {
  log "Creating deployment '${DEPLOYMENT_NAME}' on fleet '${FLEET_ID}'..."

  # Build the repeated --agent arguments from the agent specs. The CLI's
  # --agent value uses the same <agentType>:<version>:<configVersionId> format
  # this script accepts as positional args, so specs pass straight through.
  # NOTE: assumes --agent is a repeatable flag for multiple agents. Verify with
  #       `newrelic fleetcontrol deployment create --help` if this ever changes.
  local agent_args=()
  for spec in "$@"; do
    if [[ "${spec}" != *:*:* ]]; then
      log "ERROR: Invalid agent spec '${spec}'. Expected <agentType>:<version>:<configVersionId>" >&2
      exit 1
    fi
    agent_args+=(--agent "${spec}")
  done

  local response
  response=$(
    newrelic fleetcontrol deployment create \
      --fleet-id "${FLEET_ID}" \
      --name "${DEPLOYMENT_NAME}" \
      "${agent_args[@]}"
  )

  # Surface the raw CLI response for debugging.
  echo "${response}" >&2

  local deployment_id
  deployment_id=$(echo "${response}" | jq -r '.result.id')

  if [[ -z "${deployment_id}" || "${deployment_id}" == "null" ]]; then
    log "ERROR: Failed to extract deployment ID from CLI response (see above)." >&2
    exit 1
  fi

  log "Deployment created — ID: ${deployment_id}"
  echo "${deployment_id}"
}

# ---------------------------------------------------------------------------
# Step 2: Trigger the deployment through the ring policy. The CLI monitors
# rollout progress and exits non-zero on failure, so `set -e` handles errors.
# ---------------------------------------------------------------------------
trigger_deployment() {
  local deployment_id="$1"

  log "Triggering deployment '${deployment_id}' (rings: ${RINGS_TO_DEPLOY})..."

  newrelic fleetcontrol deployment deploy \
    --deployment-id "${deployment_id}" \
    --rings-to-deploy "${RINGS_TO_DEPLOY}"

  log "Deployment triggered — confirmed ID: ${deployment_id}"
}

main() {
  log "======================================="
  log "Fleet Deployment Script"
  log "======================================="
  log "Environment  : ${ENVIRONMENT} (region: ${NEW_RELIC_REGION})"
  log "Fleet ID     : ${FLEET_ID}"
  log "Deployment   : ${DEPLOYMENT_NAME}"
  log "Agents       : $*"
  log "======================================="

  ensure_newrelic_cli

  # Step 1 — Create the deployment definition
  local deployment_id
  deployment_id=$(create_fleet_deployment "$@")

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
