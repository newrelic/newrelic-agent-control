use std::collections::HashMap;
use std::sync::Arc;

use crate::cli::create_temp_file;
use http::header::AUTHORIZATION;
use http::{status, HeaderMap};
use httpmock::Method::POST;
use httpmock::MockServer;
use newrelic_super_agent::opamp::auth::config::{AuthConfig, LocalConfig, ProviderConfig};
use newrelic_super_agent::opamp::auth::token_retriever::TokenRetrieverImpl;
use newrelic_super_agent::opamp::http::builder::{HttpClientBuilder, UreqHttpClientBuilder};
use newrelic_super_agent::super_agent::config::OpAMPClientConfig;
use nr_auth::authenticator::Response;
use opamp_client::http::http_client::HttpClient;
use tempfile::TempDir;
use url::Url;

const RS256_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
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

const _RS256_PUBLIC_KEY: &str = r#"-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAtj2oIV5g+0nLcBx5JBd8
g5AZXpm9alireiKPBluzpWEietBwybSV60N8KCHCoK9WppmlxNWjJYlB1i3JS74e
cdNHunjaU65yhKV64Xtd/VTNkK4BsKdoJTR7bUxG0xMSwR0hHGOrceoOGQ96SJSi
2KalsAqPCLFLryacB3h0BvKybeySnkIO9Q/JmXT1lafB2n5vXv2bOQkswOQhdG7o
ywUUBpYDB0SK+4aV12d0aHolBQvsltFTL1X4Xd21h8eCFctyuq7rGwMtM1vclLGm
BrPkB8yrZjJU/fpF2D6HzGfinKRboBTcmo7mUF2wuYARj/IsuEklh0gz7rseIf7G
5QIDAQAB
-----END PUBLIC KEY-----
"#;

// This test verifies that empty Auth config doesn't inject any token.
// This is a temporal behavior until auth config is mandatory.
#[tokio::test(flavor = "multi_thread")]
async fn test_empty_auth_config() {
    let opamp_server = MockServer::start();
    // returns status 200 if auth header is not present
    let opamp_server_mock = opamp_server.mock(|when, then| {
        when.method(POST).path("/").matches(|req| {
            let headers: HashMap<String, String> =
                req.headers.to_owned().unwrap().into_iter().collect();
            !headers.contains_key(AUTHORIZATION.as_str())
        });
        then.status(200);
    });

    let config = OpAMPClientConfig {
        endpoint: Url::parse(opamp_server.url("/").to_string().as_str()).unwrap(),
        headers: HeaderMap::default(),
        auth_config: None,
    };

    let token_retriever = Arc::new(TokenRetrieverImpl::try_from(config.clone()).unwrap());

    let http_client_builder = UreqHttpClientBuilder::new(config, token_retriever);
    let http_client = http_client_builder.build();

    // Make the post request which shouldn't include the auth header.
    let response = http_client.unwrap().post("".into()).unwrap();
    assert_eq!(status::StatusCode::OK, response.status());
    opamp_server_mock.assert();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_non_empty_auth_header_is_injected() {
    let token = "fake_token";

    // Fake auth server that returns a token
    let auth_server = MockServer::start();
    let auth_response = Response {
        access_token: token.to_string(),
        token_type: "fake_token_type".to_string(),
        expires_in: 10,
    };
    let _ = auth_server.mock(|when, then| {
        when.method(POST);
        then.json_body(serde_json::to_value(auth_response.clone()).unwrap());
    });

    // returns status 200 if auth header contains the expected token
    let opamp_server = MockServer::start();
    let opamp_server_mock = opamp_server.mock(|when, then| {
        when.method(POST)
            .header(AUTHORIZATION.as_str(), format!("Bearer {}", token))
            .path("/");
        then.status(200);
    });

    let dir = TempDir::new().unwrap();
    let private_key_path = create_temp_file(&dir, "priv_key", RS256_PRIVATE_KEY).unwrap();

    let config = OpAMPClientConfig {
        endpoint: Url::parse(opamp_server.url("/").to_string().as_str()).unwrap(),
        headers: HeaderMap::default(),
        auth_config: Some(AuthConfig {
            client_id: "fake".into(),
            token_url: Url::parse(auth_server.url("/").as_str()).unwrap(),
            provider: ProviderConfig::Local(LocalConfig { private_key_path }),
        }),
    };

    let token_retriever = Arc::new(TokenRetrieverImpl::try_from(config.clone()).unwrap());

    let http_client_builder = UreqHttpClientBuilder::new(config, token_retriever);
    let http_client = http_client_builder.build();

    // Make the post request which must include the token
    let response = http_client.unwrap().post("".into()).unwrap();
    assert_eq!(status::StatusCode::OK, response.status());
    opamp_server_mock.assert()
}
