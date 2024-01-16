//! GCP instance id detector implementation
use http::HeaderValue;
use std::time::Duration;

use thiserror::Error;

use crate::cloud::http_client::{HttpClient, HttpClientError, HttpClientUreq};
use crate::cloud::GCP_INSTANCE_ID;
use crate::{cloud::AWS_INSTANCE_ID, DetectError, Detector, Key, Resource, Value};

use super::metadata::{GCPMetadata, IPV4_METADATA_ENDPOINT};

/// The `GCPDetector` struct encapsulates an HTTP client used to retrieve the instance metadata.
pub struct GCPDetector<C: HttpClient> {
    http_client: C,
}

const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

impl Default for GCPDetector<HttpClientUreq> {
    fn default() -> Self {
        Self {
            http_client: HttpClientUreq::new(
                IPV4_METADATA_ENDPOINT.to_string(),
                DEFAULT_CLIENT_TIMEOUT,
            ),
        }
    }
}

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum GCPDetectorError {
    /// Internal HTTP error
    #[error("`{0}`")]
    HttpError(#[from] HttpClientError),
    /// Error while deserializing endpoint metadata
    #[error("`{0}`")]
    JsonError(#[from] serde_json::Error),
    /// Unsuccessful HTTP response.
    #[error("Status code: `{0}` Canonical reason: `{1}`")]
    UnsuccessfulResponse(u16, String),
}

impl<C> Detector for GCPDetector<C>
where
    C: HttpClient,
{
    fn detect(&self) -> Result<Resource, DetectError> {
        let mut response = self
            .http_client
            .get()
            .map_err(GCPDetectorError::HttpError)?;

        response.headers_mut().append(
            "Metadata-Flavor",
            HeaderValue::from_str("Google").expect("Header value \"Google\" failed to be computed"),
        );

        // return error if status code is not within 200-299.
        if !response.status().is_success() {
            return Err(DetectError::GCPError(
                GCPDetectorError::UnsuccessfulResponse(
                    response.status().as_u16(),
                    response
                        .status()
                        .canonical_reason()
                        .unwrap_or_default()
                        .to_string(),
                ),
            ));
        }

        let metadata: GCPMetadata =
            serde_json::from_slice(response.body()).map_err(GCPDetectorError::JsonError)?;

        Ok(Resource::new([(
            Key::from(GCP_INSTANCE_ID),
            Value::from(metadata.instance_id.to_string()),
        )]))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cloud::http_client::test::MockHttpClientMock;
    use http::Response;

    #[test]
    fn detect_gcp_metadata() {
        let mut client_mock = MockHttpClientMock::new();
        client_mock.expect_get().once().returning(|| {
            Ok(Response::from(
                http::Response::builder()
                    .status(200)
                    .body(
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
    "#
                        .as_bytes()
                        .to_vec(),
                    )
                    .unwrap(),
            ))
        });

        let detector = GCPDetector {
            http_client: client_mock,
        };

        let identifiers = detector.detect().unwrap();

        assert_eq!(
            "6331980990053453154".to_string(),
            String::from(identifiers.get(GCP_INSTANCE_ID.into()).unwrap())
        )
    }

    #[test]
    fn detect_internal_http_error() {
        let mut client_mock = MockHttpClientMock::new();
        client_mock.expect_get().once().returning(|| {
            Ok(Response::from(
                http::Response::builder()
                    .status(404)
                    .body(r#""#.as_bytes().to_vec())
                    .unwrap(),
            ))
        });

        let detector = GCPDetector {
            http_client: client_mock,
        };

        let result = detector.detect();

        match result {
            Err(e) => assert_eq!(
                "error detecting gcp resources `Status code: `404` Canonical reason: `Not Found``"
                    .to_string(),
                e.to_string()
            ),
            _ => unreachable!(),
        }
    }

    #[test]
    fn detect_json_error() {
        let mut client_mock = MockHttpClientMock::new();
        client_mock.expect_get().once().returning(|| {
            Ok(Response::from(
                http::Response::builder()
                    .status(200)
                    .body(r#"{ this is an invalid json right }"#.as_bytes().to_vec())
                    .unwrap(),
            ))
        });

        let detector = GCPDetector {
            http_client: client_mock,
        };

        let result = detector.detect();

        match result {
            Err(e) => assert_eq!(
                "error detecting gcp resources ``key must be a string at line 1 column 3``"
                    .to_string(),
                e.to_string()
            ),
            _ => unreachable!(),
        }
    }
}
