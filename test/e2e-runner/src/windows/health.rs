use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::common::test::TestResult;

/// Response from the agent-control HTTP endpoint
#[derive(Debug, Serialize, Deserialize)]
struct StatusResponse {
    agent_control: AgentControlStatus,
}

/// Agent-control status information
#[derive(Debug, Serialize, Deserialize)]
struct AgentControlStatus {
    healthy: bool,
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
        return Err(format!("request was not successful: [{status_code}] {body}").into());
    }
    let status: StatusResponse = serde_json::from_str(&body)
        .map_err(|e| format!("Failed to parse JSON: {e}. Body: {body}"))?;
    if !status.agent_control.healthy {
        return Err(format!("agent-control is not healthy yet. Body: {body}").into());
    }

    Ok(body)
}
