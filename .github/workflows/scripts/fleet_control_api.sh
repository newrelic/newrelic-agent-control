#!/bin/bash
# Fleet Control Test Controller API client script
#
# This script triggers Fleet Control tests and polls for completion.
# It replicates the logic from test/e2e-runner/src/linux/scenarios/fleet_control.rs
#
# Required environment variables:
#   FLEET_CONTROL_TOKEN - Bearer token for authentication
#   FLEET_ID           - Fleet ID to run tests against
#
# Exit codes:
#   0 - Tests completed successfully (HTTP 200)
#   1 - Tests failed (HTTP 450) or other error

set -euo pipefail

readonly BASE_URL="https://fleet-management-e2e-test-runner.staging-service.newrelic.com"
readonly CLIENT_TIMEOUT=30
readonly STATUS_INITIAL_WAIT=300  # 5 minutes
readonly STATUS_POLL_INTERVAL=30
readonly STATUS_TIMEOUT=600       # 10 minutes

FLEET_CONTROL_TOKEN="${FLEET_CONTROL_TOKEN:?FLEET_CONTROL_TOKEN is required}"
FLEET_ID="${FLEET_ID:?FLEET_ID is required}"

# Trigger Fleet Control tests and return the test run ID
trigger_tests() {
    local url="${BASE_URL}/test-runner/trigger-suites"

    local request_body
    request_body=$(jq -n \
        --arg fleet_id "$FLEET_ID" \
        '{
            includeTestTags: ["FLEET_DEPLOYMENT"],
            excludeTestTags: [],
            includeParameterTags: [],
            excludeParameterTags: [],
            debugRun: false,
            allowHiddenTests: false,
            testThreads: 1,
            userDefinedArgs: {
                DeploymentServicesTestSuite: {
                    "k8s-fleet": $fleet_id
                }
            }
        }')

    echo "🚀 Triggering Fleet Control tests for fleet ID: ${FLEET_ID}" >&2

    local response
    local http_code
    local body

    response=$(curl -s -w "\n%{http_code}" \
        --max-time "$CLIENT_TIMEOUT" \
        -X POST "$url" \
        -H "Authorization: Bearer ${FLEET_CONTROL_TOKEN}" \
        -H "Content-Type: application/json" \
        -d "$request_body")

    http_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | sed '$d')

    if [[ "$http_code" == "200" ]]; then
        local test_run_id
        test_run_id=$(echo "$body" | jq -r '.testRunId')
        echo "✅ Successfully triggered test suite (HTTP 200). Run ID: ${test_run_id}" >&2
        echo "$test_run_id"
        return 0
    else
        echo "❌ Failed to trigger tests with HTTP ${http_code}. Response: ${body}" >&2
        return 1
    fi
}

# Poll for test completion
wait_for_completion() {
    local test_run_id="$1"
    local url="${BASE_URL}/test-runner/status/${test_run_id}"

    echo "⏳ Waiting ${STATUS_INITIAL_WAIT}s before checking status..." >&2
    sleep "$STATUS_INITIAL_WAIT"

    local start_time
    start_time=$(date +%s)
    echo "📊 Polling for test run ${test_run_id} completion (Timeout: ${STATUS_TIMEOUT}s)..." >&2

    while true; do
        local elapsed=$(($(date +%s) - start_time))

        if (( elapsed >= STATUS_TIMEOUT )); then
            echo "❌ Timeout reached after ${elapsed}s waiting for tests to complete" >&2
            return 1
        fi

        local response
        local http_code
        local body

        response=$(curl -s -w "\n%{http_code}" \
            --max-time "$CLIENT_TIMEOUT" \
            -X GET "$url" \
            -H "Authorization: Bearer ${FLEET_CONTROL_TOKEN}")

        http_code=$(echo "$response" | tail -n1)

        body=$(echo "$response" | sed '$d')

        case "$http_code" in
            404)
                echo "⏳ [${elapsed}s] Run not found / initializing (404). Retrying..." >&2
                sleep "$STATUS_POLL_INTERVAL"
                ;;
            204)
                echo "🏃 [${elapsed}s] Tests are running (204). Retrying..." >&2
                sleep "$STATUS_POLL_INTERVAL"
                ;;
            200)
                local pretty_response
                pretty_response=$(echo "$body" | jq -C '.' 2>/dev/null || echo "$body")
                echo "✅ [${elapsed}s] Tests completed successfully (200)!" >&2
                echo "Response: ${pretty_response}" >&2
                return 0
                ;;
            450)
                local pretty_response
                pretty_response=$(echo "$body" | jq -C '.' 2>/dev/null || echo "$body")
                echo "❌ [${elapsed}s] Tests failed (450). Response: ${pretty_response}" >&2
                return 1
                ;;
            *)
                local pretty_response
                pretty_response=$(echo "$body" | jq -C '.' 2>/dev/null || echo "$body")
                echo "❌ [${elapsed}s] Unexpected status code: ${http_code}. Response: ${pretty_response}" >&2
                return 1
                ;;
        esac
    done
}

# Main execution
main() {
    echo "========================================" >&2
    echo "Fleet Control E2E Test Runner" >&2
    echo "========================================" >&2

    # Trigger tests with retry
    local test_run_id
    local retry_count=0
    local max_retries=3

    while (( retry_count < max_retries )); do
        if test_run_id=$(trigger_tests); then
            break
        fi

        retry_count=$((retry_count + 1))
        if (( retry_count < max_retries )); then
            echo "⚠️ Retry ${retry_count}/${max_retries} in 5 seconds..." >&2
            sleep 5
        else
            echo "❌ Failed to trigger tests after ${max_retries} attempts" >&2
            exit 1
        fi
    done

    # Wait for completion
    if wait_for_completion "$test_run_id"; then
        echo "========================================" >&2
        echo "✅ Fleet Control E2E test completed successfully" >&2
        echo "========================================" >&2
        exit 0
    else
        echo "========================================" >&2
        echo "❌ Fleet Control E2E test failed" >&2
        echo "========================================" >&2
        exit 1
    fi
}

main "$@"
