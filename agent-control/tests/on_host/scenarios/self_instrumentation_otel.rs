use crate::on_host::cli::cmd_with_config_file;
use crate::on_host::tools::config::create_file;
use httpmock::Method::POST;
use httpmock::MockServer;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
};
use newrelic_agent_control::on_host::file_store::build_config_name;
use tempfile::TempDir;

const API_KEY_HEADER: &str = "api-key";
const API_KEY_VALUE: &str = "test-api-key";

#[test]
#[ignore = "requires root"]
fn self_instrumentation_otel_exports_logs_and_metrics_as_root() {
    let server = MockServer::start();

    let logs_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/logs")
            .header(API_KEY_HEADER, API_KEY_VALUE);
        then.status(200);
    });
    let metrics_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/metrics")
            .header(API_KEY_HEADER, API_KEY_VALUE);
        then.status(200);
    });

    let dir = TempDir::new().unwrap();
    let endpoint = server.base_url();

    let config = format!(
        r#"
host_id: integration-test
agents: {{}}
server:
  enabled: false
uptime_report:
  interval: 200ms
self_instrumentation:
  opentelemetry:
    endpoint: {endpoint}
    headers:
      {API_KEY_HEADER}: {API_KEY_VALUE}
    metrics:
      enabled: true
      interval: 200ms
    logs:
      enabled: true
      batch_config:
        scheduled_delay: 500ms
        max_size: 1
"#
    );
    create_file(
        config,
        dir.path()
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID)
            .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG).as_str()),
    );

    // The binary runs until a timeout kills it.
    let mut cmd = cmd_with_config_file(dir.path());
    let output = cmd.output().expect("running newrelic-agent-control binary");

    let ac_stdout = String::from_utf8_lossy(&output.stdout);
    let ac_stderr = String::from_utf8_lossy(&output.stderr);

    eprintln!("--- agent-control stdout ---\n{ac_stdout}");
    eprintln!("--- agent-control stderr ---\n{ac_stderr}");

    let logs_hits = logs_mock.calls();
    let metrics_hits = metrics_mock.calls();
    assert!(
        logs_hits >= 1,
        "expected at least one POST /v1/logs, got {logs_hits}"
    );
    assert!(
        metrics_hits >= 1,
        "expected at least one POST /v1/metrics, got {metrics_hits}"
    );
}

/// Covers `self_instrumentation.opentelemetry.traces.enabled`, which the test above does not
/// exercise. The binary creates real tracing spans on startup (e.g. `start_agent_control`);
/// with traces enabled those spans must be exported as OTLP spans to `/v1/traces`.
#[test]
#[ignore = "requires root"]
fn self_instrumentation_otel_exports_traces_as_root() {
    let server = MockServer::start();

    let traces_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/traces")
            .header(API_KEY_HEADER, API_KEY_VALUE);
        then.status(200);
    });
    // Negative controls: metrics/logs are left disabled (the struct default), so these
    // paths must see zero traffic even though the otel layer as a whole is active.
    let logs_mock = server.mock(|when, then| {
        when.method(POST).path("/v1/logs");
        then.status(200);
    });
    let metrics_mock = server.mock(|when, then| {
        when.method(POST).path("/v1/metrics");
        then.status(200);
    });

    let dir = TempDir::new().unwrap();
    let endpoint = server.base_url();

    let config = format!(
        r#"
host_id: integration-test
agents: {{}}
server:
  enabled: false
uptime_report:
  interval: 200ms
self_instrumentation:
  opentelemetry:
    endpoint: {endpoint}
    headers:
      {API_KEY_HEADER}: {API_KEY_VALUE}
    traces:
      enabled: true
      batch_config:
        scheduled_delay: 500ms
        max_size: 1
"#
    );
    create_file(
        config,
        dir.path()
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID)
            .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG).as_str()),
    );

    let mut cmd = cmd_with_config_file(dir.path());
    let output = cmd.output().expect("running newrelic-agent-control binary");

    let ac_stdout = String::from_utf8_lossy(&output.stdout);
    let ac_stderr = String::from_utf8_lossy(&output.stderr);

    eprintln!("--- agent-control stdout ---\n{ac_stdout}");
    eprintln!("--- agent-control stderr ---\n{ac_stderr}");

    let traces_hits = traces_mock.calls();
    let logs_hits = logs_mock.calls();
    let metrics_hits = metrics_mock.calls();
    assert!(
        traces_hits >= 1,
        "expected at least one POST /v1/traces, got {traces_hits}"
    );
    assert_eq!(
        logs_hits, 0,
        "logs.enabled was not set; expected zero POST /v1/logs, got {logs_hits}"
    );
    assert_eq!(
        metrics_hits, 0,
        "metrics.enabled was not set; expected zero POST /v1/metrics, got {metrics_hits}"
    );
}
