use crate::common::FleetControlApiArgs;
use crate::common::test::{TestResult, retry_panic};
use reqwest::Url;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tracing::{debug, info};

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TriggerTestResponse {
    test_run_id: String,
}

const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

const STATUS_INITIAL_WAIT: Duration = Duration::from_secs(300); // 5 minutes
const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(30);
const STATUS_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes
const FLEET_CONTROL_TEST_CONTROLLER_ENDPOINT: &str =
    "https://fleet-management-e2e-test-runner.staging-service.newrelic.com";

/// Runs Fleet Control API interaction (trigger tests and poll for completion).
///
/// This function only handles the Fleet Control API communication and does not
/// install or configure Agent Control. Useful when AC is already deployed externally
/// (e.g., in a minikube cluster).
pub fn run_fleet_control_api(args: FleetControlApiArgs) {
    info!("Starting Fleet Control API E2E test");

    trigger_and_wait_for_fleet_control_tests(
        &args.fleet_id,
        &args.fleet_control_token,
        &args.fleet_type,
    );
}

/// Triggers Fleet Control tests via API and waits for completion.
///
/// This is the core API interaction logic shared by both the full fleet-control
/// test (which installs AC first) and the fleet-control-api command (which assumes
/// AC is already deployed externally).
pub fn trigger_and_wait_for_fleet_control_tests(
    fleet_id: &str,
    fleet_control_token: &str,
    fleet_type: &str,
) {
    info!("Triggering Fleet Control tests");
    info!("Fleet ID: {fleet_id}");
    info!("Fleet type: {fleet_type}");

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
                fleet_type,
            )
        },
    );

    // Wait for completion
    retry_panic(
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

    info!("✅ Fleet Control tests completed successfully");
}

/// Triggers Fleet Control tests and returns the test run ID
fn trigger_fleet_control_tests(
    base_url: &str,
    token: &str,
    fleet_id: &str,
    fleet_type: &str,
) -> TestResult<String> {
    let client = Client::builder().timeout(CLIENT_TIMEOUT).build()?;
    let url = Url::parse(base_url)?
        .join("test-runner/")?
        .join("trigger-suites")?;

    let request_body = TriggerTestRequest {
        include_test_tags: vec!["FLEET_DEPLOYMENT".to_string()],
        test_threads: 1,
        user_defined_args: serde_json::json!({
            "DeploymentServicesTestSuite": {
                fleet_type: fleet_id
            }
        }),
        ..TriggerTestRequest::default()
    };
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
) -> TestResult<()> {
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
            404 => {
                info!("⏳ [{elapsed_secs} s] Run not found / initializing (404). Retrying...");
                std::thread::sleep(STATUS_POLL_INTERVAL);
            }
            204 => {
                info!("🏃 [{elapsed_secs} s] Tests are running (204). Retrying...");
                std::thread::sleep(STATUS_POLL_INTERVAL);
            }
            200 => {
                let response = serde_json::to_string_pretty(&response.json::<Value>()?)?;
                info!("✅ [{elapsed_secs} s] Tests completed successfully (200)!");
                info!("Response: {response}");
                break Ok(());
            }
            450 => {
                let response = serde_json::to_string_pretty(&response.json::<Value>()?)?;
                Err(format!(
                    "❌ [{elapsed_secs} s] Tests failed (450). Response: {response}"
                ))?;
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
