//! AWS EC2 instance id detector implementation

use super::metadata::AWSMetadata;
use crate::cloud::aws::http_client::AWSHttpClient;
use crate::cloud::http_client::{HttpClient, HttpClientError};
use crate::{DetectError, Detector, Key, Resource, Value, cloud::AWS_INSTANCE_ID};
use core::str;
use std::time::Duration;
use thiserror::Error;
use tracing::instrument;

/// The default AWS instance metadata endpoint.
pub const AWS_IPV4_METADATA_ENDPOINT: &str =
    "http://169.254.169.254/latest/dynamic/instance-identity/document";
/// The default AWS instance metadata token endpoint.
pub const AWS_IPV4_METADATA_TOKEN_ENDPOINT: &str = "http://169.254.169.254/latest/api/token";

pub(crate) const TTL_TOKEN_DEFAULT: Duration = Duration::from_secs(10);

/// The `AWSDetector` struct encapsulates an HTTP client used to retrieve the instance metadata.
pub struct AWSDetector<C: HttpClient> {
    aws_http_client: AWSHttpClient<C>,
}

impl<C: HttpClient> AWSDetector<C> {
    /// Returns a new instance of AWSDetector
    pub fn new(aws_http_client: AWSHttpClient<C>) -> Self {
        Self { aws_http_client }
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
    #[instrument(skip_all, name = "detect_aws")]
    fn detect(&self) -> Result<Resource, DetectError> {
        let response = self
            .aws_http_client
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
mod tests {
    use super::*;
    use crate::cloud::http_client::tests::MockHttpClient;
    use assert_matches::assert_matches;

    #[test]
    fn detect_aws_metadata() {
        let mut client_mock = MockHttpClient::new();

        client_mock.expect_send().once().returning(|_| {
            Ok(http::Response::builder()
                .status(200)
                .body(r#" "#.as_bytes().to_vec())
                .unwrap())
        });

        client_mock.expect_send().once().returning(|_| {
            Ok(http::Response::builder()
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
                .unwrap())
        });

        let aws_http_client = AWSHttpClient::new(
            client_mock,
            "/metadata".to_string(),
            "/token".to_string(),
            TTL_TOKEN_DEFAULT,
        );

        let detector = AWSDetector { aws_http_client };

        let identifiers = detector.detect().unwrap();

        assert_eq!(
            "i-1234567890abcdef0".to_string(),
            String::from(identifiers.get(AWS_INSTANCE_ID.into()).unwrap())
        )
    }

    #[test]
    fn detect_internal_http_error() {
        let mut client_mock = MockHttpClient::new();

        client_mock.expect_send().once().returning(|_| {
            Ok(http::Response::builder()
                .status(200)
                .body(r#""#.as_bytes().to_vec())
                .unwrap())
        });

        client_mock.expect_send().once().returning(|_| {
            Ok(http::Response::builder()
                .status(404)
                .body(r#""#.as_bytes().to_vec())
                .unwrap())
        });

        let aws_http_client = AWSHttpClient::new(
            client_mock,
            "/metadata".to_string(),
            "/token".to_string(),
            TTL_TOKEN_DEFAULT,
        );

        let detector = AWSDetector { aws_http_client };

        let result = detector.detect();

        assert_matches!(
            result,
            Err(DetectError::AWSError(
                AWSDetectorError::UnsuccessfulResponse(404, _)
            ))
        );
    }

    #[test]
    fn detect_json_error() {
        let mut client_mock = MockHttpClient::new();

        client_mock.expect_send().once().returning(|_| {
            Ok(http::Response::builder()
                .status(200)
                .body(r#" "#.as_bytes().to_vec())
                .unwrap())
        });

        client_mock.expect_send().once().returning(|_| {
            Ok(http::Response::builder()
                .status(200)
                .body(r#"{ this is an invalid json right }"#.as_bytes().to_vec())
                .unwrap())
        });

        let aws_http_client = AWSHttpClient::new(
            client_mock,
            "/metadata".to_string(),
            "/token".to_string(),
            TTL_TOKEN_DEFAULT,
        );

        let detector = AWSDetector { aws_http_client };

        let result = detector.detect();

        assert_matches!(
            result,
            Err(DetectError::AWSError(AWSDetectorError::JsonError(_)))
        );
    }
}
