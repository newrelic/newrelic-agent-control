#!/usr/bin/env bash
# Downloads the latest resolved Instrumentation Metadata, selects the latest versions and exports them.
# Required env: GH_TOKEN, ARTIFACT_NAME, ACTION_PATH. Optional env: PLATFORM (linux, windows, kubernetes; default from the runner OS)
set -euo pipefail

MAX_AGE_HOURS=12

if [ -z "${PLATFORM:-}" ]; then
  case "$RUNNER_OS" in
    Linux) PLATFORM=linux ;;
    Windows) PLATFORM=windows ;;
    *) echo "::error::No default platform for runner OS $RUNNER_OS, set the platform input"; exit 1 ;;
  esac
fi
case "$PLATFORM" in
  linux) target="HOST/LINUX" ;;
  windows) target="HOST/WINDOWS" ;;
  kubernetes) target="KUBERNETES" ;;
  *) echo "::error::platform must be linux, windows or kubernetes, got '$PLATFORM'"; exit 1 ;;
esac

artifact=$(gh api "repos/$GITHUB_REPOSITORY/actions/artifacts?name=$ARTIFACT_NAME&per_page=20" \
  --jq '[.artifacts[] | select(.expired==false and .workflow_run.head_branch=="main")][0]')
if [ -z "$artifact" ] || [ "$artifact" = "null" ]; then
  echo "::error::No non-expired '$ARTIFACT_NAME' artifact found on main; run the scheduled_resolve_agent_versions workflow"
  exit 1
fi

download_dir="${RUNNER_TEMP}/agent-versions"
data_file="$download_dir/agent-versions.json"
# A previous call in the same job (e.g. another platform) already downloaded the snapshot
if [ ! -f "$data_file" ]; then
  mkdir -p "$download_dir"
  gh run download "$(jq -r '.workflow_run.id' <<<"$artifact")" --repo "$GITHUB_REPOSITORY" \
    --name "$ARTIFACT_NAME" --dir "$download_dir"
fi

age_hours=$(jq -r 'now - (.fetched_at | fromdate) | . / 3600 | floor' "$data_file")
if [ "$age_hours" -ge "$MAX_AGE_HOURS" ]; then
  echo "::error::Agent versions snapshot is ${age_hours}h old (max ${MAX_AGE_HOURS}h); the resolve workflow is failing"
  exit 1
fi

resolved=$(jq -c --arg target "$target" --argjson agents "$(jq -c . "$ACTION_PATH/agents.json")" \
  --from-file "$ACTION_PATH/select.jq" "$data_file" 2>&1) || {
  echo "::error::Selecting versions failed: $resolved"
  exit 1
}

jq -r '.[] | "\(.name)_VERSION=\(.packageVersion)", "\(.name)_AT_VERSION=\(.agentTypeVersion)"' <<<"$resolved" >>"$GITHUB_ENV"
echo "json=$resolved" >>"$GITHUB_OUTPUT"
echo "::group::Resolved versions"
jq . <<<"$resolved"
echo "::endgroup::"
jq -r '.[] | "\(.name) \(.packageVersion) (agent-type \(.agentTypeVersion))"' <<<"$resolved"
