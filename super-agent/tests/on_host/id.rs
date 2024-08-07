use httpmock::Method::GET;
use httpmock::MockServer;
use newrelic_super_agent::opamp::instance_id::IdentifiersProvider;
use resource_detection::cloud::cloud_id::detector::CloudIdDetector;
use resource_detection::system::detector::SystemDetector;

const UNRESPOSIVE_METADATA_ENDPOINT: &str = "http://localhost:9999";

#[test]
#[cfg(target_family = "unix")]
fn test_aws_cloud_id() {
    use crate::on_host::consts::AWS_VM_RESPONSE;

    let metadata_path = "/latest/meta-data/instance-id";
    let instance_id = "i-123456787d725bbe7";

    let fake_metadata_server = MockServer::start();
    let mock = fake_metadata_server.mock(|when, then| {
        when.method(GET).path(metadata_path);
        then.status(200)
            .header("content-type", "application/json")
            .body(AWS_VM_RESPONSE);
    });

    let id = IdentifiersProvider::new(
        SystemDetector::default(),
        CloudIdDetector::new(
            fake_metadata_server.url(metadata_path),
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
        ),
    )
    .provide()
    .unwrap();

    assert_eq!(id.cloud_instance_id, instance_id);

    mock.assert_hits(1);
}
#[test]
#[cfg(target_family = "unix")]
fn test_azure_cloud_id() {
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

    let id = IdentifiersProvider::new(
        SystemDetector::default(),
        CloudIdDetector::new(
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
            fake_metadata_server.url(metadata_path),
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
        ),
    )
    .provide()
    .unwrap();

    assert_eq!(id.cloud_instance_id, instance_id);

    mock.assert_hits(1);
}

#[test]
#[cfg(target_family = "unix")]
fn test_gcp_cloud_id() {
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

    let id = IdentifiersProvider::new(
        SystemDetector::default(),
        CloudIdDetector::new(
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
            UNRESPOSIVE_METADATA_ENDPOINT.to_string(),
            fake_metadata_server.url(metadata_path),
        ),
    )
    .provide()
    .unwrap();

    assert_eq!(id.cloud_instance_id, instance_id);

    mock.assert_hits(1);
}
