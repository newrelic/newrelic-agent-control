use serde::{Deserialize, Serialize};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info};

use crate::tools::test::TestResult;

/// Response from the agent-control HTTP endpoint
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub agent_control: AgentControlStatus,
}

/// Agent-control status information
#[derive(Debug, Serialize, Deserialize)]
pub struct AgentControlStatus {
    pub healthy: bool,
}

/// Checks the health of agent-control via its HTTP status endpoint.
pub fn check_health(endpoint: &str) -> TestResult<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let resp = client
        .get(endpoint)
        .send()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status_code = resp.status();
    let body = resp.text()?;

    if !status_code.is_success() {
        return Err(format!("request was not successful: [{}] {}", status_code, body).into());
    }

    let status: StatusResponse = serde_json::from_str(&body)
        .map_err(|e| format!("Failed to parse JSON: {}. Body: {}", e, body))?;

    if !status.agent_control.healthy {
        return Err(format!("agent-control is not healthy yet. Body: {}", body).into());
    }

    Ok(body)
}

/// Checks the health of agent-control with retry logic.
pub fn check_health_with_retry(
    endpoint: &str,
    max_attempts: u32,
    retry_delay: Duration,
) -> TestResult<String> {
    let start_time = Instant::now();

    for attempt in 1..=max_attempts {
        debug!(
            attempt = attempt,
            max_attempts = max_attempts,
            endpoint = endpoint,
            "Checking status endpoint"
        );

        match check_health(endpoint) {
            Ok(status) => {
                info!("agent-control is healthy");
                return Ok(status);
            }
            Err(e) => {
                debug!(attempt = attempt, error = %e, "Attempt failed");
                if attempt < max_attempts {
                    debug!(retry_delay = ?retry_delay, "Retrying...");
                    thread::sleep(retry_delay);
                }
            }
        }
    }

    let duration = start_time.elapsed();
    Err(format!(
        "health-check failed after {} attempts over {:?}",
        max_attempts, duration
    )
    .into())
}
