#!/bin/bash
# Reports one AgentControlStats event to New Relic; appends a warning to WARNINGS_FILE
# if the binary grew >10%. Sending that warning to Slack is the caller's job, not this script's.
#
# BINARY_NAME and BINARY_TARGET are derived from BINARY_PATH, assuming the goreleaser
# layout dist/<build-id>_<target>/<binary>.
#
# Required env: BINARY_PATH, BINARY_VERSION, NR_ACCOUNT_ID, NR_LICENSE_KEY
# Optional env: NR_USER_API_KEY (NerdGraph, skips growth check if unset), WARNINGS_FILE (default: binary-size-warnings.txt)

set -euo pipefail

GROWTH_THRESHOLD_PCT=10

if [[ -z "${BINARY_PATH:-}" ]]; then
  echo "BINARY_PATH is required" >&2
  exit 1
fi

# Only nightly (schedule/workflow_dispatch) and real pre-releases (release) build the binary
# the same way, so only they are size-comparable. Gate on this explicitly instead of relying
# on which callers happen to be wired with NR secrets.
TRACKED_TRIGGER_EVENTS=("schedule" "workflow_dispatch" "release")
if ! printf '%s\n' "${TRACKED_TRIGGER_EVENTS[@]}" | grep -qx "${GITHUB_EVENT_NAME:-}"; then
  echo "GITHUB_EVENT_NAME=${GITHUB_EVENT_NAME:-<unset>} is not a tracked release pipeline, skipping binary size report for ${BINARY_PATH}"
  exit 0
fi

if [[ -z "${NR_ACCOUNT_ID:-}" || -z "${NR_LICENSE_KEY:-}" ]]; then
  echo "NR_ACCOUNT_ID/NR_LICENSE_KEY not set, skipping binary size report for ${BINARY_PATH}"
  exit 0
fi

BINARY_NAME=$(basename "$BINARY_PATH" .exe)
BINARY_TARGET_DIR=$(basename "$(dirname "$BINARY_PATH")")
BINARY_TARGET="${BINARY_TARGET_DIR#*_}"

SIZE_BYTES=$(stat -c%s "$BINARY_PATH")

# GITHUB_HEAD_REF is set for PRs; GITHUB_REF_NAME covers push/schedule/dispatch.
BRANCH="${GITHUB_HEAD_REF:-${GITHUB_REF_NAME:-}}"

# Fetch the previous size before reporting the current one, so we don't race our own insert.
PREVIOUS_SIZE_BYTES=""
if [[ -n "${NR_USER_API_KEY:-}" ]]; then
  nrql_query="SELECT sizeBytes FROM AgentControlStats WHERE binaryName = '${BINARY_NAME}' AND target = '${BINARY_TARGET}' SINCE 90 days ago ORDER BY timestamp DESC LIMIT 1"
  graphql_query="{ actor { account(id: ${NR_ACCOUNT_ID}) { nrql(query: \"${nrql_query}\") { results } } } }"
  request_body=$(jq -n --arg query "$graphql_query" '{query: $query}')

  nerdgraph_response=$(curl -s -X POST "https://api.newrelic.com/graphql" \
    -H "Content-Type: application/json" \
    -H "API-Key: ${NR_USER_API_KEY}" \
    -d "$request_body")

  PREVIOUS_SIZE_BYTES=$(echo "$nerdgraph_response" | jq -r '.data.actor.account.nrql.results[0].sizeBytes // empty')
fi

event=$(jq -n \
  --arg binaryName   "$BINARY_NAME" \
  --arg target       "$BINARY_TARGET" \
  --arg version      "$BINARY_VERSION" \
  --arg commit       "$GITHUB_SHA" \
  --arg branch       "$BRANCH" \
  --arg runId        "$GITHUB_RUN_ID" \
  --arg triggerEvent "$GITHUB_EVENT_NAME" \
  --argjson sizeBytes "$SIZE_BYTES" \
  '[{
    eventType:    "AgentControlStats",
    binaryName:   $binaryName,
    target:       $target,
    version:      $version,
    commit:       $commit,
    branch:       $branch,
    runId:        $runId,
    triggerEvent: $triggerEvent,
    sizeBytes:    $sizeBytes
  }]')

curl -s -X POST \
  "https://insights-collector.newrelic.com/v1/accounts/${NR_ACCOUNT_ID}/events" \
  -H "Content-Type: application/json" \
  -H "Api-Key: ${NR_LICENSE_KEY}" \
  -d "$event"

if [[ -z "$PREVIOUS_SIZE_BYTES" ]]; then
  echo "No previous build found for ${BINARY_NAME} (${BINARY_TARGET}), skipping growth check"
elif [[ "$PREVIOUS_SIZE_BYTES" == "0" ]]; then
  echo "Previous build for ${BINARY_NAME} (${BINARY_TARGET}) reported size 0 bytes, skipping growth check"
else
  growth_pct=$(awk -v cur="$SIZE_BYTES" -v prev="$PREVIOUS_SIZE_BYTES" \
    'BEGIN { printf "%.2f", (cur - prev) * 100.0 / prev }')

  if awk -v p="$growth_pct" -v t="$GROWTH_THRESHOLD_PCT" 'BEGIN { exit !(p > t) }'; then
    prev_mb=$(awk -v b="$PREVIOUS_SIZE_BYTES" 'BEGIN { printf "%.2f", b / 1000000 }')
    cur_mb=$(awk -v b="$SIZE_BYTES" 'BEGIN { printf "%.2f", b / 1000000 }')
    warning="Agent Control binary \`${BINARY_NAME}\` (${BINARY_TARGET}) grew ${growth_pct}% versus its previous build: ${prev_mb} MB -> ${cur_mb} MB (version ${BINARY_VERSION})"
    echo "Binary size regression: $warning"
    echo "$warning" >> "${WARNINGS_FILE:-binary-size-warnings.txt}"
  fi
fi
