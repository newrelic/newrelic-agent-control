use std::net::TcpListener;

use newrelic_super_agent::opamp::instance_id::IdentifiersProvider;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// TODO: run only on Linux
#[tokio::test]
async fn test_cloud_id() {
    let mock_server = MockServer::builder()
        .listener(TcpListener::bind("127.0.0.1:4343").unwrap())
        .start()
        .await;

    // AWS metadata endpoint mock
    let aws_mock = Mock::given(method("GET"))
        .and(path("/aws_testing_metadata_endpoint"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"
{
  "accountId": "012345678901",
  "architecture": "x86_64",
  "availabilityZone": "eu-central-1c",
  "billingProducts": null,
  "devpayProductCodes": null,
  "marketplaceProductCodes": null,
  "imageId": "ami-01ff76477b9b30d59",
  "instanceId": "i-123456787d725bbe7",
  "instanceType": "t3a.nano",
  "kernelId": null,
  "pendingTime": "2022-06-20T09:51:52Z",
  "privateIp": "172.29.40.136",
  "ramdiskId": null,
  "region": "eu-central-1",
  "version": "2017-09-30"
}
"#,
            "application/json",
        ));
    let aws_mock_guard = mock_server.register_as_scoped(aws_mock).await;

    // let aws_detector = AWSDetector::default();
    //
    // let id = aws_detector.detect().unwrap();
    // Create a mock on the server
    let id = IdentifiersProvider::default().provide().unwrap();

    assert!(id.cloud_instance_id == "i-123456787d725bbe7".to_string());
    drop(aws_mock_guard);

    // GCP metadata endpoint mock
    let endpoint_mock = Mock::given(method("GET"))
        .and(path("/aws_testing_metadata_endpoint"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"
{
		"attributes": {},
		"cpuPlatform": "Intel Haswell",
		"description": "",
		"disks": [
			{
				"deviceName": "mmacias-micro",
				"index": 0,
				"mode": "READ_WRITE",
				"type": "PERSISTENT"
			}
		],
		"hostname": "mmacias-micro.c.beyond-181918.internal",
		"id": 6331980990053453154,
		"image": "projects/debian-cloud/global/images/debian-9-stretch-v20171025",
		"licenses": [
			{
				"id": "1000205"
			}
		],
		"machineType": "projects/260890654058/machineTypes/f1-micro",
		"maintenanceEvent": "NONE",
		"name": "mmacias-micro",
		"networkInterfaces": [
			{
				"accessConfigs": [
					{
						"externalIp": "104.154.137.202",
						"type": "ONE_TO_ONE_NAT"
					}
				],
				"forwardedIps": [],
				"ip": "10.128.0.5",
				"ipAliases": [],
				"mac": "42:01:0a:80:00:05",
				"network": "projects/260890654058/networks/default",
				"targetInstanceIps": []
			}
		],
		"preempted": "FALSE",
		"scheduling": {
			"automaticRestart": "TRUE",
			"onHostMaintenance": "MIGRATE",
			"preemptible": "FALSE"
		},
		"serviceAccounts": {},
		"tags": [],
		"virtualClock": {
			"driftToken": "0"
		},
		"zone": "projects/260890654058/zones/us-central1-c"
	}
"#,
            "application/json",
        ));
    let mock_guard = mock_server.register_as_scoped(endpoint_mock).await;

    // let aws_detector = AWSDetector::default();
    //
    // let id = aws_detector.detect().unwrap();
    // Create a mock on the server
    let id = IdentifiersProvider::default().provide().unwrap();

    assert!(id.cloud_instance_id == "6331980990053453154".to_string());
    drop(mock_guard);
}
