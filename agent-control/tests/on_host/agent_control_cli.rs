//! Integration tests for newrelic-agent-control-onhost-cli

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

#[test]
fn test_config_generator_fleet_disabled_proxy() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("output.yaml").to_string_lossy().to_string();

    let mut cmd = cargo_bin_cmd!("newrelic-agent-control-cli");
    let args = format!(
        "generate-config 
          --fleet-disabled 
          --region us
          --proxy-url https://some.proxy.url/ 
          --proxy-ca-bundle-dir /test/bundle/dir 
          --ignore-system-proxy true 
          --output-path {path}",
    );
    cmd.args(args.split_ascii_whitespace());
    cmd.assert().success();

    let expected_yaml = format!(
        r#"
server:
  enabled: true
proxy:
  url: https://some.proxy.url/
  ca_bundle_dir: /test/bundle/dir
  ignore_system_proxy: true
{LOG_SECTION}
    "#,
    );
    let expected_value: serde_yaml::Value = serde_yaml::from_str(&expected_yaml).unwrap();
    let actual_content = std::fs::read_to_string(&path).unwrap();
    let actual_value: serde_yaml::Value = serde_yaml::from_str(&actual_content).unwrap();
    assert_eq!(actual_value, expected_value);
}

#[test]
fn test_config_generator_fleet_disabled_proxy_empty_fields() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("output.yaml").to_string_lossy().to_string();

    let mut cmd = cargo_bin_cmd!("newrelic-agent-control-cli");
    let args = format!(
        "generate-config 
          --fleet-disabled 
          --region us
          --proxy-url= 
          --proxy-ca-bundle-dir= 
          --proxy-ca-bundle-file= 
          --ignore-system-proxy= 
          --output-path {path}",
    );
    cmd.args(args.split_ascii_whitespace());
    cmd.assert().success();

    let expected_yaml = format!(
        r#"
server:
  enabled: true
{LOG_SECTION}
    "#,
    );
    let expected_value: serde_yaml::Value = serde_yaml::from_str(&expected_yaml).unwrap();
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

    let mut cmd = cargo_bin_cmd!("newrelic-agent-control-cli");
    let args = format!(
        "generate-config 
          --region us
          --fleet-id FLEET-ID 
          --auth-client-id CLIENT-ID 
          --auth-private-key-path {key_path} 
          --output-path {path}",
    );
    cmd.args(args.split_ascii_whitespace());
    cmd.assert().success();

    let expected_yaml = format!(
        r#"
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

{LOG_SECTION}
    "#,
    );
    let expected_value: serde_yaml::Value = serde_yaml::from_str(&expected_yaml).unwrap();
    let actual_content = std::fs::read_to_string(&path).unwrap();
    let actual_value: serde_yaml::Value = serde_yaml::from_str(&actual_content).unwrap();
    assert_eq!(actual_value, expected_value);
}

#[test]
fn test_config_generator_environment_variables() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("config.yaml").to_string_lossy().to_string();
    let env_vars_path = tmp.path().join("env.yaml").to_string_lossy().to_string();

    let mut cmd = cargo_bin_cmd!("newrelic-agent-control-cli");
    let args = format!(
        "generate-config 
          --fleet-disabled 
          --region us 
          --output-path {path}
          --env-vars-file-path {env_vars_path} 
          --newrelic-license-key fake_license",
    );
    cmd.args(args.split_ascii_whitespace());
    cmd.assert().success();

    let expected_value: serde_yaml::Value = serde_yaml::from_str(
        r#"
OTEL_EXPORTER_OTLP_ENDPOINT: https://otlp.nr-data.net:4317/
NEW_RELIC_LICENSE_KEY: fake_license
    "#,
    )
    .unwrap();
    let actual_content = std::fs::read_to_string(&env_vars_path).unwrap();
    let actual_value: serde_yaml::Value = serde_yaml::from_str(&actual_content).unwrap();
    assert_eq!(actual_value, expected_value);
}

#[cfg(target_family = "windows")]
const LOG_SECTION: &str = r#"
log:
  file:
    enabled: true
"#;

#[cfg(target_family = "unix")]
const LOG_SECTION: &str = "";
