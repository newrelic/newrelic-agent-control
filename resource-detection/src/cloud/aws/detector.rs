//! AWS EC2 instance id detector implementation
use std::time::Duration;

use thiserror::Error;

use crate::cloud::http_client::{HttpClient, HttpClientError, HttpClientUreq};
use crate::{cloud::AWS_INSTANCE_ID, DetectError, Detector, Key, Resource, Value};

use super::metadata::{AWSMetadata, IPV4_METADATA_ENDPOINT};

/// The `AWSDetector` struct encapsulates an HTTP client used to retrieve the instance metadata.
pub struct AWSDetector<C: HttpClient> {
    http_client: C,
}

const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

impl Default for AWSDetector<HttpClientUreq> {
    fn default() -> Self {
        Self {
            http_client: HttpClientUreq::new(
                IPV4_METADATA_ENDPOINT.to_string(),
                DEFAULT_CLIENT_TIMEOUT,
                None,
            ),
        }
    }
}

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum AWSDetectorError {
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

impl<C> Detector for AWSDetector<C>
where
    C: HttpClient,
{
    fn detect(&self) -> Result<Resource, DetectError> {
        let response = self
            .http_client
            .get()
            .map_err(AWSDetectorError::HttpError)?;

        // return error if status code is not within 200-299.
        if !response.status().is_success() {
            return Err(DetectError::AWSError(
                AWSDetectorError::UnsuccessfulResponse(
                    response.status().as_u16(),
                    response
                        .status()
                        .canonical_reason()
                        .unwrap_or_default()
                        .to_string(),
                ),
            ));
        }

        let metadata: AWSMetadata =
            serde_json::from_slice(response.body()).map_err(AWSDetectorError::JsonError)?;

        Ok(Resource::new([(
            Key::from(AWS_INSTANCE_ID),
            Value::from(metadata.instance_id),
        )]))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cloud::http_client::test::MockHttpClientMock;
    use http::Response;

    #[test]
    fn detect_aws_metadata() {
        let mut client_mock = MockHttpClientMock::new();
        client_mock.expect_get().once().returning(|| {
            Ok(Response::from(
                http::Response::builder()
                    .status(200)
                    .body(
                        r#"
    {
        "devpayProductCodes" : null,
        "marketplaceProductCodes" : [ "1abc2defghijklm3nopqrs4tu" ],
        "availabilityZone" : "us-west-2b",
        "privateIp" : "10.158.112.84",
        "version" : "2017-09-30",
        "instanceId" : "i-1234567890abcdef0",
        "billingProducts" : null,
        "instanceType" : "t2.micro",
        "accountId" : "123456789012",
        "imageId" : "ami-5fb8c835",
        "pendingTime" : "2016-11-19T16:32:11Z",
        "architecture" : "x86_64",
        "kernelId" : null,
        "ramdiskId" : null,
        "region" : "us-west-2"
    }
    "#
                        .as_bytes()
                        .to_vec(),
                    )
                    .unwrap(),
            ))
        });

        let detector = AWSDetector {
            http_client: client_mock,
        };

        let identifiers = detector.detect().unwrap();

        assert_eq!(
            "i-1234567890abcdef0".to_string(),
            String::from(identifiers.get(AWS_INSTANCE_ID.into()).unwrap())
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

        let detector = AWSDetector {
            http_client: client_mock,
        };

        let result = detector.detect();

        match result {
            Err(e) => assert_eq!(
                "error detecting aws resources `Status code: `404` Canonical reason: `Not Found``"
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

        let detector = AWSDetector {
            http_client: client_mock,
        };

        let result = detector.detect();

        match result {
            Err(e) => assert_eq!(
                "error detecting aws resources ``key must be a string at line 1 column 3``"
                    .to_string(),
                e.to_string()
            ),
            _ => unreachable!(),
        }
    }
}
