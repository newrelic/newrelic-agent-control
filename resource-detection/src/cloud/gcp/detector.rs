//! GCP instance id detector implementation
use super::metadata::GCPMetadata;
use crate::cloud::http_client::{HttpClient, HttpClientError};
use crate::cloud::GCP_INSTANCE_ID;
use crate::{DetectError, Detector, Key, Resource, Value};
use http::{HeaderMap, Request};
use thiserror::Error;

/// Default GCP instance metadata endpoint.
pub const GCP_IPV4_METADATA_ENDPOINT: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/?recursive=true";

/// The `GCPDetector` struct encapsulates an HTTP client used to retrieve the instance metadata.
pub struct GCPDetector<C: HttpClient> {
    http_client: C,
    metadata_endpoint: String,
    headers: HeaderMap,
}

const HEADER_KEY: &str = "Metadata-Flavor";
const HEADER_VALUE: &str = "Google";

impl<C: HttpClient> GCPDetector<C> {
    /// Returns a new instance of GCPDetector
    pub fn try_new(http_client: C, metadata_endpoint: String) -> Result<Self, HttpClientError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            HEADER_KEY,
            HEADER_VALUE.parse().expect("constant valid value"),
        );

        Ok(Self {
            http_client,
            metadata_endpoint,
            headers,
        })
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
        let mut request = Request::builder()
            .method("GET")
            .uri(self.metadata_endpoint.to_string())
            .body(Vec::new())?;
        request.headers_mut().extend(self.headers.clone());
        let response = self.http_client.send(request)?;

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

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::cloud::http_client::tests::MockHttpClientMock;
//     use assert_matches::assert_matches;

//     #[test]
//     fn detect_gcp_metadata() {
//         let mut client_mock = MockHttpClientMock::new();
//         client_mock.expect_get().once().returning(|| {
//             Ok(http::Response::builder()
//                 .status(200)
//                 .body(
//                     r#"
// {
// 		"attributes": {},
// 		"cpuPlatform": "Intel Haswell",
// 		"description": "",
// 		"disks": [
// 			{
// 				"deviceName": "mmacias-micro",
// 				"index": 0,
// 				"mode": "READ_WRITE",
// 				"type": "PERSISTENT"
// 			}
// 		],
// 		"hostname": "mmacias-micro.c.beyond-181918.internal",
// 		"id": 6331980990053453154,
// 		"image": "projects/debian-cloud/global/images/debian-9-stretch-v20171025",
// 		"licenses": [
// 			{
// 				"id": "1000205"
// 			}
// 		],
// 		"machineType": "projects/260890654058/machineTypes/f1-micro",
// 		"maintenanceEvent": "NONE",
// 		"name": "mmacias-micro",
// 		"networkInterfaces": [
// 			{
// 				"accessConfigs": [
// 					{
// 						"externalIp": "104.154.137.202",
// 						"type": "ONE_TO_ONE_NAT"
// 					}
// 				],
// 				"forwardedIps": [],
// 				"ip": "10.128.0.5",
// 				"ipAliases": [],
// 				"mac": "42:01:0a:80:00:05",
// 				"network": "projects/260890654058/networks/default",
// 				"targetInstanceIps": []
// 			}
// 		],
// 		"preempted": "FALSE",
// 		"scheduling": {
// 			"automaticRestart": "TRUE",
// 			"onHostMaintenance": "MIGRATE",
// 			"preemptible": "FALSE"
// 		},
// 		"serviceAccounts": {},
// 		"tags": [],
// 		"virtualClock": {
// 			"driftToken": "0"
// 		},
// 		"zone": "projects/260890654058/zones/us-central1-c"
// 	}
//     "#
//                     .as_bytes()
//                     .to_vec(),
//                 )
//                 .unwrap())
//         });
//
//         let detector = GCPDetector {
//             http_client: client_mock,
//             metadata_endpoint: "".to_string(),
//         };
//
//         let identifiers = detector.detect().unwrap();
//
//         assert_eq!(
//             "6331980990053453154".to_string(),
//             String::from(identifiers.get(GCP_INSTANCE_ID.into()).unwrap())
//         )
//     }
//
//     #[test]
//     fn detect_internal_http_error() {
//         let mut client_mock = MockHttpClientMock::new();
//         client_mock.expect_get().once().returning(|| {
//             Ok(http::Response::builder()
//                 .status(404)
//                 .body(r#""#.as_bytes().to_vec())
//                 .unwrap())
//         });
//
//         let detector = GCPDetector {
//             http_client: client_mock,
//             metadata_endpoint: "".to_string(),
//         };
//
//         let result = detector.detect();
//
//         assert_matches!(
//             result,
//             Err(DetectError::GCPError(
//                 GCPDetectorError::UnsuccessfulResponse(404, _)
//             ))
//         );
//     }
//
//     #[test]
//     fn detect_json_error() {
//         let mut client_mock = MockHttpClientMock::new();
//         client_mock.expect_get().once().returning(|| {
//             Ok(http::Response::builder()
//                 .status(200)
//                 .body(r#"{ this is an invalid json right }"#.as_bytes().to_vec())
//                 .unwrap())
//         });
//
//         let detector = GCPDetector {
//             http_client: client_mock,
//             metadata_endpoint: "".to_string(),
//         };
//
//         let result = detector.detect();
//
//         assert_matches!(
//             result,
//             Err(DetectError::GCPError(GCPDetectorError::JsonError(_)))
//         );
//     }
// }
