use std::net::TcpListener;

use newrelic_super_agent::opamp::instance_id::IdentifiersProvider;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// test_cloud_id tests that the default IdentifiersProvider is able to retrieve the cloud instance
// id for any cloud provider. It spawns a mock server and provides the mocked cloud metadata in the
// endpoint specified by the TEST_IPV4_METADATA_ENDPOINT environment variable. Required format of
// the TEST_IPV4_METADATA_ENDPOINT: "http://address/endpoint". For example: "http://127.0.0.1:4343/testing_metadata_endpoint"
// Note that if the environment variable is not defined, the test will fail.
// only unix: /etc/machine-id
#[cfg(target_family = "unix")]
#[tokio::test]
async fn test_cloud_id() {
    let (mut address, endpoint) =
        konst::option::unwrap!(option_env!("TEST_IPV4_METADATA_ENDPOINT"))
            .rsplit_once("/")
            .expect("Not a valid endpoint, expected format: http://address/endpoint");
    address = address
        .strip_prefix("http://")
        .expect("Not a valid endpoint, expected format: http://address/endpoint");

    let mock_server = MockServer::builder()
        .listener(TcpListener::bind(address).unwrap())
        .start()
        .await;

    let cloud_data = [
        (super::consts::AWS_VM_RESPONSE, "i-123456787d725bbe7"),
        (super::consts::GCP_VM_RESPONSE, "6331980990053453154"),
        (
            super::consts::AZURE_VM_RESPONSE,
            "02aab8a4-74ef-476e-8182-f6d2ba4166a7",
        ),
    ];

    for (metadata_endpoint, expected_cloud_id) in cloud_data {
        let endpoint_mock = Mock::given(method("GET")).and(path(endpoint)).respond_with(
            ResponseTemplate::new(200).set_body_raw(metadata_endpoint, "application/json"),
        );
        let mock_guard = mock_server.register_as_scoped(endpoint_mock).await;

        let id = IdentifiersProvider::default().provide().unwrap();

        assert!(id.cloud_instance_id == expected_cloud_id.to_string());
        drop(mock_guard);
    }
}
