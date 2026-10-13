#!/usr/bin/env bash
# Queries Instrumentation Metadata and writes the raw data to $OUTPUT_FILE.
# Required env: IM_TOKEN, IM_URL, OUTPUT_FILE
set -euo pipefail

QUERY='query($filter: AgentDefinitionFilter!, $cursor: String, $orderBy: [AgentDefinitionOrderBy!]) {
  actor {
    agentDefinitions(filter: $filter, cursor: $cursor, orderBy: $orderBy) {
      nextCursor
      items {
        agentType agentVersion enabled eolDate
        configurationDefinitions { configurationType configurationVersion }
      }
    }
  }
}'

fetch_target() {
  local filter="$1" cursor="" items="[]" payload response
  while :; do
    payload=$(jq -n --arg query "$QUERY" --argjson filter "$filter" --arg cursor "$cursor" '{
      query: $query,
      variables: {
        filter: $filter,
        cursor: (if $cursor == "" then null else $cursor end),
        orderBy: [{field: "AGENT_TYPE", direction: "ASC"}, {field: "AGENT_VERSION", direction: "DESC"}]
      }
    }')
    response=$(curl -sS --fail --retry 5 --retry-all-errors --retry-delay 5 -X POST "$IM_URL" \
      -H "Content-Type: application/json" -H "Authorization: Bearer $IM_TOKEN" --data "$payload")
    {
      echo "::group::Instrumentation Metadata response ($filter, cursor '${cursor}')"
      jq . <<<"$response"
      echo "::endgroup::"
    } >&2
    if [ "$(jq 'has("errors")' <<<"$response")" = "true" ]; then
      echo "::error::Instrumentation Metadata returned errors: $(jq -c .errors <<<"$response")" >&2
      exit 1
    fi
    items=$(jq -c --argjson page "$(jq -c '.data.actor.agentDefinitions.items' <<<"$response")" '. + $page' <<<"$items")
    cursor=$(jq -r '.data.actor.agentDefinitions.nextCursor // ""' <<<"$response")
    [ -n "$cursor" ] || break
  done
  echo "$items"
}

linux=$(fetch_target '{"platform": {"eq": "HOST"}, "operatingSystem": {"eq": "LINUX"}}')
windows=$(fetch_target '{"platform": {"eq": "HOST"}, "operatingSystem": {"eq": "WINDOWS"}}')
kubernetes=$(fetch_target '{"platform": {"eq": "KUBERNETESCLUSTER"}}')

jq -n --argjson linux "$linux" --argjson windows "$windows" --argjson kubernetes "$kubernetes" \
  '{fetched_at: (now | todate), targets: {"HOST/LINUX": $linux, "HOST/WINDOWS": $windows, "KUBERNETES": $kubernetes}}' \
  >"$OUTPUT_FILE"

echo "::group::Resolved agent versions snapshot"
jq . "$OUTPUT_FILE"
echo "::endgroup::"
