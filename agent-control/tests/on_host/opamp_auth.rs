use assert_cmd::Command;
use http::header::{AUTHORIZATION, CONTENT_TYPE};
use httpmock::Method::POST;
use httpmock::{MockServer, When};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use newrelic_agent_control::agent_control::defaults::AGENT_CONTROL_CONFIG_FILE;
use nr_auth::authenticator::{Request, Response};
use nr_auth::jwt::claims::Claims;
use nr_auth::token_retriever::DEFAULT_AUDIENCE;
use predicates::prelude::predicate;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;

#[cfg(unix)]
#[test]
fn test_auth_local_provider_as_root() {
    use crate::on_host::cli::create_temp_file;

    let token = "fakeToken";

    let dir = TempDir::new().unwrap();
    let private_key_path = create_temp_file(&dir, "priv_key", RS256_PRIVATE_KEY).unwrap();

    let auth_server = auth_server(token.to_string());

    // returns status 200 if auth header contains the expected token
    let opamp_server = MockServer::start();
    let opamp_server_mock = opamp_server.mock(|when, then| {
        when.method(POST)
            .header(AUTHORIZATION.as_str(), format!("Bearer {}", token))
            .path("/");
        then.status(200);
    });

    let config_path = create_temp_file(
        &dir,
        AGENT_CONTROL_CONFIG_FILE,
        format!(
            r#"
fleet_control:
  enabled: true
  endpoint: "{}"
  auth_config:
    token_url: "{}"
    client_id: "fake"
    provider: "local"
    private_key_path: "{}"
log:
  level: debug
agents: {{}}
"#,
            opamp_server.url("/"),
            auth_server.url(TOKEN_PATH),
            private_key_path.to_str().unwrap()
        )
        .as_str(),
    )
    .unwrap();

    let mut cmd = cmd_agent_control(config_path);
    // cmd_assert is not made for long running programs, so we kill it.
    // Enough time for the SA to start and send at least 1 AgentToServer OpAMP message.
    cmd.timeout(Duration::from_secs(10));

    let output = cmd
        .assert()
        .try_interrupted()
        .expect("shouldn't have crashed")
        .get_output()
        .to_owned();

    println!("stdout:\n {}", String::from_utf8(output.stdout).unwrap(),);

    assert!(opamp_server_mock.calls() >= 1)
}

// This test verifies that empty Auth config doesn't inject any token.
// This is a temporal behavior until auth config is mandatory.
#[cfg(unix)]
#[test]
fn test_empty_auth_config_as_root() {
    use crate::on_host::cli::create_temp_file;

    let dir = TempDir::new().unwrap();

    let opamp_server = MockServer::start();
    let opamp_server_mock = opamp_server.mock(|when, then| {
        when.method(POST).path("/").is_true(|req| {
            let headers = req.headers();
            !headers.contains_key(AUTHORIZATION.as_str()) && headers.contains_key("api-key")
        });
        then.status(200);
    });

    let config_path = create_temp_file(
        &dir,
        AGENT_CONTROL_CONFIG_FILE,
        format!(
            r#"
fleet_control:
  enabled: true
  endpoint: "{}"
  headers:
    api-key: "fakeKey"
log:
  level: debug
agents: {{}}
"#,
            opamp_server.url("/"),
        )
        .as_str(),
    )
    .unwrap();

    let mut cmd = cmd_agent_control(config_path);
    // Enough time for the SA to start and send at least 1 AgentToServer OpAMP message.
    cmd.timeout(Duration::from_secs(1));

    let output = cmd
        .assert()
        .try_interrupted()
        .expect("shouldn't have crashed")
        .get_output()
        .to_owned();

    println!("stdout:\n {}", String::from_utf8(output.stdout).unwrap(),);

    assert!(opamp_server_mock.calls() >= 1)
}

#[cfg(unix)]
#[test]
fn test_unauthorized_token_retrieve_as_root() {
    use super::cli::create_temp_file;

    let dir = TempDir::new().unwrap();
    let private_key_path = create_temp_file(&dir, "priv_key", RS256_PRIVATE_KEY).unwrap();

    let auth_server = MockServer::start();

    let _ = auth_server.mock(|when, then| {
        when.method(POST).path(TOKEN_PATH);
        then.status(401);
    });

    let config_path = create_temp_file(
        &dir,
        AGENT_CONTROL_CONFIG_FILE,
        format!(
            r#"
fleet_control:
  enabled: true
  endpoint: "https://localhost"
  auth_config:
    token_url: "{}"
    client_id: "fake"
    provider: "local"
    private_key_path: "{}"
log:
  level: debug
agents: {{}}
"#,
            auth_server.url(TOKEN_PATH),
            private_key_path.to_str().unwrap()
        )
        .as_str(),
    )
    .unwrap();

    let mut cmd = cmd_agent_control(config_path);
    // This timeout has been added so we can discriminate if the agent-control has crashed or not.
    // if timed out means that the agent-control haven't crashed and that's not expected.
    // This is checked on the assert.
    cmd.timeout(Duration::from_secs(30));

    let assert = cmd.assert();

    // the agent-control stops the execution.
    assert
        .try_interrupted()
        .expect_err("should have failure before timeout")
        .assert()
        .failure()
        .stdout(predicate::str::is_match(r".*ERROR.*errors happened creating headers.*").unwrap());
}

fn cmd_agent_control(config_path: PathBuf) -> Command {
    let mut cmd = Command::cargo_bin("newrelic-agent-control").unwrap();
    cmd.arg("--local-dir").arg(config_path.parent().unwrap());
    cmd
}

fn auth_server(token: String) -> MockServer {
    // Fake auth server that returns a token
    let mock_server = MockServer::start();

    let _ = mock_server.mock(|when, then| {
        when.method(POST)
            .path(TOKEN_PATH)
            .header(CONTENT_TYPE.as_str(), "application/json")
            .and(is_authorized);
        then.json_body(
            serde_json::to_value(Response {
                access_token: token,
                token_type: "bearer".to_string(),
                expires_in: 10,
            })
            .unwrap(),
        );
    });

    mock_server
}
fn is_authorized(when: When) -> When {
    when.is_true(|req| {
        let request: Request = serde_json::from_slice(req.body_ref()).unwrap();

        // Validation
        let mut validation = Validation::new(Algorithm::RS256);
        validation.sub = Some(request.client_id.to_owned());
        validation.set_audience(&[DEFAULT_AUDIENCE]);
        validation.set_required_spec_claims(&["exp", "sub", "aud"]);

        // Decode the signed token
        jsonwebtoken::decode::<Claims>(
            &request.client_assertion,
            &DecodingKey::from_rsa_pem(RS256_PUBLIC_KEY.as_bytes()).unwrap(),
            &validation,
        )
        .is_ok()
    })
}

pub const RS256_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEuwIBADANBgkqhkiG9w0BAQEFAASCBKUwggShAgEAAoIBAQC2PaghXmD7Sctw
HHkkF3yDkBlemb1qWKt6Io8GW7OlYSJ60HDJtJXrQ3woIcKgr1ammaXE1aMliUHW
LclLvh5x00e6eNpTrnKEpXrhe139VM2QrgGwp2glNHttTEbTExLBHSEcY6tx6g4Z
D3pIlKLYpqWwCo8IsUuvJpwHeHQG8rJt7JKeQg71D8mZdPWVp8Hafm9e/Zs5CSzA
5CF0bujLBRQGlgMHRIr7hpXXZ3RoeiUFC+yW0VMvVfhd3bWHx4IVy3K6rusbAy0z
W9yUsaYGs+QHzKtmMlT9+kXYPofMZ+KcpFugFNyajuZQXbC5gBGP8iy4SSWHSDPu
ux4h/sblAgMBAAECggEAFu48ptA3jz7qknV+t7Ad2ncJ/imFmClGkFRjXzcwLE3D
1yS9oF+w4nyoFWukD/BoDIf2QAVqpRk3d8Hkm3t1XLirRJcaz586aR7iTpdljO/7
+qmubEIwPEg1hJvtqHb0q+hkp2wSIUAEXJpiNlo/gFe9ruAxPbSDU6tdxCHfpZTz
SlZSa0mwcAuKVuq6chdtLurvvVTLatI2/Avg22tkVRfjyGe4NKNak3N09htmtt3k
nxzsDz229Ho7Qw0lEU/Rpo60p/1UFSLH5Kdsycc33cF0ACznAQ3pWozkwXVR0TfF
rmUFX73/zZfI3/expjuk3HTUZ/6W4mHgZZA6oqUHAQKBgQDbQsCr/SxdtrKx8VL5
xwMIxamVxePkKH9+P3m+bw8xaT6buyrX1Y/kkyyEBqRd9W6iiKEFF7h1Or2uKjqh
5WoKPASh8AFVtAeTgtWQWRN2+iLt4jTIxnbzeUiNFCLY5hFTZnpM64vkOeNx1lfd
Uhet30/x35TRgbyU2pIQ9lOz5QKBgQDUxuzzTLnXKDbRd3fxLhnqNMuz2PUvAkTQ
zyuqIHHUqEMx1oFaslAlFSjX+FEhEuOqISlDZf09OYvnSRF9fz3ronm3yYGxPBVr
rwpE9lGdsy/ul1/EU3FjsVAZ0MOf+1RB69xoMrYTi9+CfEF9Ue5zqMIN/ibgyx6V
souIn2OXAQKBgQC8PKq8/TXBnr/7FHtwBPMN7OSSuLnVfw81i7kxTJd2jCw79ovp
kGdgjRmCn1EteS/qSfIzNRIfUrbVd1uu8g3/i1dOz4XV1iFK+t/udQrI8iZapAE8
/WXR0SYAOHFSVPI675e/wdjvruMdMC9uyrOZikZQGOrikscb5CnSdieWIQJ/S6Jq
mBGt/c1NryfIevLoQ1iBEG0OuqcTzyXVX6Qo0m79c7nMQXEhDA15d0vNivQr+U3Q
XSTj39+U26IdlX6lhB09Jxd6AoZZFu4huGHWoTgQ0b79S8xdghKFZqfO4g904/nz
XxanoksWKEwC+4kkOfjDAjZVm5KYTJ4q+2WtAQKBgHeeQfmvoCzCpPpZD/K2SxGD
sJWFhh7HSGFHc3C9bJ5KrftA0o64SeXEGSnxFQJ2oGrLqlZuyfdJ0NsDI9kQVWnM
USEqOAWZjvEBorOcB1tTO3vBgZOBz41i/x9xlYw2fmt+fTBUNAN6ABFcrEEaAIFQ
3PdAPhldn/zZaxkLJ4h1
-----END PRIVATE KEY-----
"#;

const RS256_PUBLIC_KEY: &str = r#"-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAtj2oIV5g+0nLcBx5JBd8
g5AZXpm9alireiKPBluzpWEietBwybSV60N8KCHCoK9WppmlxNWjJYlB1i3JS74e
cdNHunjaU65yhKV64Xtd/VTNkK4BsKdoJTR7bUxG0xMSwR0hHGOrceoOGQ96SJSi
2KalsAqPCLFLryacB3h0BvKybeySnkIO9Q/JmXT1lafB2n5vXv2bOQkswOQhdG7o
ywUUBpYDB0SK+4aV12d0aHolBQvsltFTL1X4Xd21h8eCFctyuq7rGwMtM1vclLGm
BrPkB8yrZjJU/fpF2D6HzGfinKRboBTcmo7mUF2wuYARj/IsuEklh0gz7rseIf7G
5QIDAQAB
-----END PUBLIC KEY-----
"#;

const TOKEN_PATH: &str = "/auth/token";
