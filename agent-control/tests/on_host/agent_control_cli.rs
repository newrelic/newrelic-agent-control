//! Integration tests for newrelic-agent-control-onhost-cli

use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn test_config_generator_fleet_disabled_proxy() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("output.yaml").to_string_lossy().to_string();

    let mut cmd = Command::cargo_bin("newrelic-agent-control-onhost-cli").unwrap();
    let args = format!(
        "generate-config --fleet-disabled --agent-set infra-agent --region us --proxy-url https://some.proxy.url/ --proxy-ca-bundle-dir /test/bundle/dir --proxy-ca-bundle-file /test/bundle/file --ignore-system-proxy --output-path {path}",
    );
    cmd.args(args.split(" "));
    cmd.assert().success();

    let expected_value: serde_yaml::Value = serde_yaml::from_str(
        r#"
server:
  enabled: true
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
proxy:
  url: https://some.proxy.url/
  ca_bundle_dir: /test/bundle/dir
  ca_bundle_file: /test/bundle/file
  ignore_system_proxy: true
    "#,
    )
    .unwrap();
    let actual_content = std::fs::read_to_string(&path).unwrap();
    let actual_value: serde_yaml::Value = serde_yaml::from_str(&actual_content).unwrap();
    assert_eq!(actual_value, expected_value);
}

#[test]
fn test_config_generator_fleet_enabled_identity_provisioned() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("output.yaml").to_string_lossy().to_string();
    let key_path = tmp.path().join("key");
    std::fs::write(&key_path, "fake-key").unwrap();
    let key_path = key_path.to_string_lossy().to_string();

    let mut cmd = Command::cargo_bin("newrelic-agent-control-onhost-cli").unwrap();
    let args = format!(
        "generate-config --agent-set infra-agent --region us --fleet-id FLEET-ID --auth-client-id CLIENT-ID --auth-private-key-path {key_path} --output-path {path}",
    );
    cmd.args(args.split(" "));
    cmd.assert().success();

    let expected_value: serde_yaml::Value = serde_yaml::from_str(
        &format!(r#"
fleet_control:
  endpoint: https://opamp.service.newrelic.com/v1/opamp
  signature_validation:
    public_key_server_url: https://publickeys.newrelic.com/r/blob-management/global/agentconfiguration/jwks.json
  fleet_id: FLEET-ID
  auth_config:
    token_url: https://system-identity-oauth.service.newrelic.com/oauth2/token
    client_id: CLIENT-ID
    provider: local
    private_key_path: {key_path}

server:
  enabled: true

agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
    "#,
        ))
    .unwrap();
    let actual_content = std::fs::read_to_string(&path).unwrap();
    let actual_value: serde_yaml::Value = serde_yaml::from_str(&actual_content).unwrap();
    assert_eq!(actual_value, expected_value);
}
