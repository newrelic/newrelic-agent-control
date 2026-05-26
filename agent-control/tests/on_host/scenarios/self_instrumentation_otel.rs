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
