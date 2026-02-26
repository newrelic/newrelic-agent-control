#!/bin/bash
# Reports a single AgentControlE2ETest custom event to the New Relic Events API.
#
# Required environment variables (set via the workflow step's `env:` block):
#   JOB_START_TIME        - Unix timestamp recorded at the start of the job (via $GITHUB_ENV)
#   E2E_SCENARIO          - Scenario name, e.g. "infra-agent"
#   E2E_ENVIRONMENT       - One of: linux, windows, k8s
#   E2E_STATUS            - job.status value: success, failure, or cancelled
#   E2E_CALLER_WORKFLOW   - Name of the top-level workflow that triggered this run
#   NR_ACCOUNT_ID         - New Relic account ID
#   NR_LICENSE_KEY        - New Relic license key (used as the Events API ingest key)
#
# Standard GitHub Actions variables (automatically injected by the runner):
#   GITHUB_EVENT_NAME, GITHUB_HEAD_REF, GITHUB_REF_NAME, GITHUB_RUN_ID

set -euo pipefail

END_TIME=$(date +%s)
DURATION=$((END_TIME - JOB_START_TIME))

# GITHUB_HEAD_REF is set for pull requests; GITHUB_REF_NAME covers push/schedule/dispatch.
BRANCH="${GITHUB_HEAD_REF:-${GITHUB_REF_NAME:-}}"

events=$(jq -n \
  --arg scenario        "$E2E_SCENARIO" \
  --arg status          "$E2E_STATUS" \
  --arg environment     "$E2E_ENVIRONMENT" \
  --arg callerWorkflow  "$E2E_CALLER_WORKFLOW" \
  --arg triggerEvent    "$GITHUB_EVENT_NAME" \
  --arg branch          "$BRANCH" \
  --arg runId           "$GITHUB_RUN_ID" \
  --argjson startTimestamp  "$JOB_START_TIME" \
  --argjson endTimestamp    "$END_TIME" \
  --argjson durationSeconds "$DURATION" \
  '[{
    eventType:       "AgentControlE2ETest",
    environment:     $environment,
    scenario:        $scenario,
    status:          $status,
    callerWorkflow:  $callerWorkflow,
    triggerEvent:    $triggerEvent,
    branch:          $branch,
    runId:           $runId,
    startTimestamp:  $startTimestamp,
    endTimestamp:    $endTimestamp,
    durationSeconds: $durationSeconds
  }]')

curl -s -X POST \
  "https://insights-collector.newrelic.com/v1/accounts/${NR_ACCOUNT_ID}/events" \
  -H "Content-Type: application/json" \
  -H "Api-Key: ${NR_LICENSE_KEY}" \
  -d "$events"
