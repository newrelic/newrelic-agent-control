use httpmock::Method::GET;
use httpmock::MockServer;
use newrelic_agent_control::http::client::HttpClient;
use newrelic_agent_control::http::config::{HttpConfig, ProxyConfig};
use resource_detection::cloud::cloud_id::detector::CloudIdDetector;
use resource_detection::cloud::http_client::DEFAULT_CLIENT_TIMEOUT;
use resource_detection::system::detector::SystemDetector;

const UNRESPONSIVE_METADATA_ENDPOINT: &str = "http://localhost:9999";

#[test]
fn test_aws_cloud_id() {
    use httpmock::Method::PUT;
    use newrelic_agent_control::opamp::instance_id::on_host::identifiers::IdentifiersProvider;

    use crate::on_host::consts::AWS_VM_RESPONSE;

    let metadata_path = "/latest/meta-data/instance-id";
    let token_path = "/token";
    let fake_token = "fake_token";
    let instance_id = "i-123456787d725bbe7";

    let fake_metadata_server = MockServer::start();
    let mock = fake_metadata_server.mock(|when, then| {
        when.method(GET).path(metadata_path);
        then.status(200)
            .header("content-type", "application/json")
            .body(AWS_VM_RESPONSE);
    });
    let token_mock = fake_metadata_server.mock(|when, then| {
        when.method(PUT).path(token_path);
        then.status(200).body(fake_token);
    });

    let http_client = HttpClient::new(HttpConfig::new(
        DEFAULT_CLIENT_TIMEOUT,
        DEFAULT_CLIENT_TIMEOUT,
        ProxyConfig::default(),
    ))
    .unwrap();

    let cloud_id_detector = CloudIdDetector::new(
        http_client.clone(),
        http_client.clone(),
        http_client,
        fake_metadata_server.url(metadata_path),
        fake_metadata_server.url(token_path),
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
    );

    let id_providers = IdentifiersProvider {
        system_detector: SystemDetector::default(),
        cloud_id_detector,
        host_id: "".to_string(),
        fleet_id: "".to_string(),
    };

    let id = id_providers.provide().unwrap();

    assert_eq!(id.cloud_instance_id, instance_id);

    mock.assert_calls(1);
    token_mock.assert_calls(1);
}
#[test]
fn test_azure_cloud_id() {
    use newrelic_agent_control::opamp::instance_id::on_host::identifiers::IdentifiersProvider;

    use crate::on_host::consts::AZURE_VM_RESPONSE;

    let metadata_path = "/metadata/instance";
    let instance_id = "02aab8a4-74ef-476e-8182-f6d2ba4166a7";

    let fake_metadata_server = MockServer::start();
    let mock = fake_metadata_server.mock(|when, then| {
        when.method(GET).path(metadata_path);
        then.status(200)
            .header("content-type", "application/json")
            .body(AZURE_VM_RESPONSE);
    });

    let http_client = HttpClient::new(HttpConfig::new(
        DEFAULT_CLIENT_TIMEOUT,
        DEFAULT_CLIENT_TIMEOUT,
        ProxyConfig::default(),
    ))
    .unwrap();

    let cloud_id_detector = CloudIdDetector::new(
        http_client.clone(),
        http_client.clone(),
        http_client,
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
        fake_metadata_server.url(metadata_path),
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
    );

    let id_providers = IdentifiersProvider {
        system_detector: SystemDetector::default(),
        cloud_id_detector,
        host_id: "".to_string(),
        fleet_id: "".to_string(),
    };

    let id = id_providers.provide().unwrap();

    assert_eq!(id.cloud_instance_id, instance_id);

    mock.assert_calls(1);
}
#[test]
fn test_gcp_cloud_id() {
    use newrelic_agent_control::opamp::instance_id::on_host::identifiers::IdentifiersProvider;

    use crate::on_host::consts::GCP_VM_RESPONSE;

    let metadata_path = "/metadata/instance";
    let instance_id = "6331980990053453154";

    let fake_metadata_server = MockServer::start();
    let mock = fake_metadata_server.mock(|when, then| {
        when.method(GET).path(metadata_path);
        then.status(200)
            .header("content-type", "application/json")
            .body(GCP_VM_RESPONSE);
    });

    let http_client = HttpClient::new(HttpConfig::new(
        DEFAULT_CLIENT_TIMEOUT,
        DEFAULT_CLIENT_TIMEOUT,
        ProxyConfig::default(),
    ))
    .unwrap();

    let cloud_id_detector = CloudIdDetector::new(
        http_client.clone(),
        http_client.clone(),
        http_client,
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
        UNRESPONSIVE_METADATA_ENDPOINT.to_string(),
        fake_metadata_server.url(metadata_path),
    );

    let id_providers = IdentifiersProvider {
        system_detector: SystemDetector::default(),
        cloud_id_detector,
        host_id: "".to_string(),
        fleet_id: "".to_string(),
    };

    let id = id_providers.provide().unwrap();

    assert_eq!(id.cloud_instance_id, instance_id);

    mock.assert_calls(1);
}
