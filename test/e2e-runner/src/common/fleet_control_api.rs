use crate::common::FleetControlArgs;
use crate::common::test::{TestResult, retry_panic};
use reqwest::Url;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Request body sent to the Fleet Control test runner to trigger a test suite.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct TriggerTestRequest {
    include_test_tags: Vec<String>,
    exclude_test_tags: Vec<String>,
    include_parameter_tags: Vec<String>,
    exclude_parameter_tags: Vec<String>,
    debug_run: bool,
    allow_hidden_tests: bool,
    test_threads: u32,
    user_defined_args: Value,
}

/// Response received after successfully triggering a Fleet Control test run.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TriggerTestResponse {
    test_run_id: String,
}

/// Maps test suite names to the list of test names within that suite.
type TestSuitesReport = HashMap<String, Vec<String>>;

/// Response returned by the Fleet Control test runner when a test run has completed.
///
/// Deserialized from JSON responses with HTTP status `200` (all passed) or `450` (at least one
/// test failed or was inconclusive). Use [`FinishedTestResponse::is_failed`] to check the outcome.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishedTestResponse {
    test_run_id: String,
    test_run_timestamp: f64,
    triggered_test_count: usize,
    debug_run: bool,
    passed_count: usize,
    pub(crate) failed_count: usize,
    pub(crate) inconclusive_count: usize,
    ignored_count: usize,
    passed_tests: TestSuitesReport,
    failed_tests: TestSuitesReport,
    inconclusive_tests: TestSuitesReport,
    ignored_tests: TestSuitesReport,
}

impl FinishedTestResponse {
    /// Determines if this test run failed.
    ///
    /// According to the docs at <https://pages.datanerd.us/site-engineering/nr-platform-docs/nr-test-runner/resource.html>,
    /// the HTTP status code of a test suite with at least 1 failed/unconclusive
    /// test should be `450`, but the caller of a function returning [`FinishedTestResponse`] might not have access to this status, so we provide this method to inspect the response type.
    pub fn is_failed(&self) -> bool {
        self.failed_count > 0 || self.inconclusive_count > 0
    }
}

/// HTTP client timeout per request.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait before polling for status after triggering a test run.
/// Fleet Control tests take several minutes to start, so we avoid busy-polling early.
const STATUS_INITIAL_WAIT: Duration = Duration::from_secs(300);
/// Interval between successive status polls once the initial wait has elapsed.
const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(30);
/// Maximum total time to wait for a test run to finish after the initial wait.
const STATUS_TIMEOUT: Duration = Duration::from_secs(600);
/// Base URL of the Fleet Control E2E test runner service (staging only).
const FLEET_CONTROL_TEST_CONTROLLER_ENDPOINT: &str =
    "https://fleet-management-e2e-test-runner.staging-service.newrelic.com";

/// Runs Fleet Control API interaction (trigger tests and poll for completion).
///
/// This function only handles the Fleet Control API communication and does not
/// install or configure Agent Control. Useful when AC is already deployed externally
/// (e.g., in a minikube cluster).
pub fn run_fleet_control_api(args: &FleetControlArgs) {
    info!("Starting Fleet Control API E2E test");

    let response = trigger_and_wait_for_fleet_control_tests(
        &args.fleet_id,
        &args.fleet_control_token,
        &args.include_test_tags,
        &args.test_scenarios,
    );

    // Write test report to JSON file
    write_test_report(&response);

    // Check if tests failed and exit with error if so
    if response.is_failed() {
        panic!(
            "❌ Tests failed: {} failed, {} inconclusive",
            response.failed_count, response.inconclusive_count
        );
    }
}

/// Triggers Fleet Control tests via API and waits for completion.
///
/// This is the core API interaction logic shared by both the full fleet-control
/// test (which installs AC first) and the fleet-control-api command (which assumes
/// AC is already deployed externally).
pub fn trigger_and_wait_for_fleet_control_tests(
    fleet_id: &str,
    fleet_control_token: &str,
    include_test_tags: &[String],
    test_scenarios: &[String],
) -> FinishedTestResponse {
    info!("Triggering Fleet Control tests");
    info!("Fleet ID: {fleet_id}");
    info!("Include test tags: {include_test_tags:?}");
    info!("Test scenarios: {test_scenarios:?}");

    // Trigger Fleet Control tests
    let test_run_id = retry_panic(
        3,
        Duration::from_secs(5),
        "trigger Fleet Control tests",
        || {
            trigger_fleet_control_tests(
                FLEET_CONTROL_TEST_CONTROLLER_ENDPOINT,
                fleet_control_token,
                fleet_id,
                include_test_tags,
                test_scenarios,
            )
        },
    );

    // Wait for completion
    let test_response = retry_panic(
        1,
        Duration::from_secs(1),
        "wait for Fleet Control tests completion",
        || {
            wait_for_fleet_control_completion(
                FLEET_CONTROL_TEST_CONTROLLER_ENDPOINT,
                fleet_control_token,
                &test_run_id,
            )
        },
    );

    info!("✅ Fleet Control test run completed successfully");
    test_response
}

/// Triggers Fleet Control tests and returns the test run ID
fn trigger_fleet_control_tests(
    base_url: &str,
    token: &str,
    fleet_id: &str,
    include_test_tags: &[String],
    test_scenarios: &[String],
) -> TestResult<String> {
    let client = Client::builder().timeout(CLIENT_TIMEOUT).build()?;
    let url = Url::parse(base_url)?
        .join("test-runner/")?
        .join("trigger-suites")?;

    let request_body = build_trigger_request(fleet_id, include_test_tags, test_scenarios);
    info!("Triggering Fleet Control tests for fleet ID: {}", fleet_id);

    debug!(payload = ?request_body, "Sending request");
    let response = client
        .post(url.as_ref())
        .bearer_auth(token)
        .json(&request_body)
        .send()?;

    let status = response.status();
    if status.is_success() {
        let run_id = response.json::<TriggerTestResponse>()?.test_run_id;
        info!("✅ Successfully triggered test suite (HTTP 200). Run ID: {run_id}");
        Ok(run_id)
    } else {
        let error_body = response
            .text()
            .unwrap_or_else(|_| "Unable to read response".to_string());
        Err(format!("❌ Failed with HTTP {status}. Response: {error_body}").into())
    }
}

/// Polls Fleet Control test status until completion or timeout
fn wait_for_fleet_control_completion(
    base_url: &str,
    token: &str,
    test_run_id: &str,
) -> TestResult<FinishedTestResponse> {
    let client = Client::builder().timeout(CLIENT_TIMEOUT).build()?;

    let url = Url::parse(base_url)?
        .join("test-runner/")?
        .join("status/")?
        .join(test_run_id)?;

    debug!("Status check URL for this test: {url}");

    info!("Waiting for {STATUS_INITIAL_WAIT:?} before checking status...");
    std::thread::sleep(STATUS_INITIAL_WAIT);

    let start_time = std::time::Instant::now();
    info!("Polling for test run {test_run_id} completion (Timeout: {STATUS_TIMEOUT:?})...");

    loop {
        let elapsed = start_time.elapsed();

        if elapsed >= STATUS_TIMEOUT {
            Err(format!(
                "❌ Timeout reached after {elapsed:?} waiting for tests to complete",
            ))?;
        }

        let response = client.get(url.as_ref()).bearer_auth(token).send()?;

        let status = response.status();
        let elapsed_secs = elapsed.as_secs();

        match status.as_u16() {
            // Provided 'testRunId' isn't found in the cache
            404 => {
                info!("⏳ [{elapsed_secs} s] Run not found / initializing (404). Retrying...");
                std::thread::sleep(STATUS_POLL_INTERVAL);
            }
            // Tests are still running
            204 => {
                info!("🏃 [{elapsed_secs} s] Tests are running (204). Retrying...");
                std::thread::sleep(STATUS_POLL_INTERVAL);
            }
            // Tests completed successfully
            200 => {
                let response = response.json::<FinishedTestResponse>()?;
                let response_str = serde_json::to_string_pretty(&response)?;
                info!("✅ [{elapsed_secs} s] Tests completed successfully (200)!");
                info!("Response: {response_str}");
                break Ok(response);
            }
            // At least 1 test failed, or was marked as inconclusive
            450 => {
                let response = response.json::<FinishedTestResponse>()?;
                let response_str = serde_json::to_string_pretty(&response)?;
                warn!("❌ [{elapsed_secs} s] Tests failed (450).");
                warn!("Response: {response_str}");
                break Ok(response);
            }
            _ => {
                let error_body = serde_json::to_string_pretty(&response.json::<Value>()?)?;
                Err(format!(
                    "❌ [{elapsed_secs} s] Unexpected status code: {status}. Response: {error_body}",
                ))?;
            }
        }
    }
}

/// Builds the request body sent to the Fleet Control test runner.
fn build_trigger_request(
    fleet_id: &str,
    include_test_tags: &[String],
    test_scenarios: &[String],
) -> TriggerTestRequest {
    let user_defined_args: serde_json::Value = test_scenarios
        .iter()
        .map(|name| (name.clone(), serde_json::json!({ "fleetId": fleet_id })))
        .collect::<serde_json::Map<_, _>>()
        .into();

    TriggerTestRequest {
        include_test_tags: include_test_tags.to_vec(),
        test_threads: 1,
        user_defined_args,
        ..TriggerTestRequest::default()
    }
}

/// Writes a flat test report derived from the API response to a JSON file.
pub fn write_test_report(response: &FinishedTestResponse) {
    let filename = "fleet-control-test-report.json";
    match std::fs::write(
        filename,
        serde_json::to_string_pretty(&response).expect("Could not render response as JSON"),
    ) {
        Ok(_) => info!("📝 Test report written to: {filename}"),
        Err(e) => error!("⚠️  Failed to write report file: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::FleetControlArgs;
    use clap::Parser;

    // E2E test from CLI args to JSON data
    #[test]
    fn trigger_request_body_matches_cli_flags() {
        let args = FleetControlArgs::try_parse_from([
            "e2e-runner",
            "--fleet-id",
            "test-fleet-id",
            "--fleet-control-token",
            "some-token",
            "--include-test-tag",
            "FLEET_DEPLOYMENT_REMOTE",
            "--test-scenario",
            "ManagedEntityIsConnectedRemote",
            "--test-scenario",
            "DeployValidConfigurationsRemote",
            "--test-scenario",
            "DeployInvalidConfigurationsRemote",
            "--test-scenario",
            "DeployMultipleConfigsRemote",
        ])
        .unwrap();

        let request = build_trigger_request(
            &args.fleet_id,
            &args.include_test_tags,
            &args.test_scenarios,
        );
        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(
            json["includeTestTags"],
            serde_json::json!(["FLEET_DEPLOYMENT_REMOTE"])
        );
        assert_eq!(json["excludeTestTags"], serde_json::json!([]));
        assert_eq!(json["testThreads"], 1);
        assert_eq!(json["debugRun"], false);
        assert_eq!(json["allowHiddenTests"], false);

        let user_args = &json["userDefinedArgs"];
        assert_eq!(
            user_args.as_object().unwrap().len(),
            4,
            "number of scenarios should match the number of items passed by CLI"
        );

        for scenario in [
            "ManagedEntityIsConnectedRemote",
            "DeployValidConfigurationsRemote",
            "DeployInvalidConfigurationsRemote",
            "DeployMultipleConfigsRemote",
        ] {
            assert_eq!(
                user_args[scenario]["fleetId"], "test-fleet-id",
                "scenario {scenario} should map to the provided fleet ID"
            );
        }
    }
}
