//! AWS EC2 instance id detector implementation
use std::time::Duration;

use thiserror::Error;

use crate::{cloud::AWS_INSTANCE_ID, Detect, DetectError, Key, Resource, Value};

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
            ),
        }
    }
}

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum HttpClientError {
    /// Represents Ureq crate error.
    #[error("internal client error: `{0}`")]
    UreqError(String),
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

/// The `HttpClient` trait defines the HTTP get interface to be implemented
/// by HTTP clients.
pub trait HttpClient {
    /// Returns a `http::Response<Vec<u8>>` structure as the HTPP response or
    /// HttpClientError if an error was found.
    fn get(&self) -> Result<http::Response<Vec<u8>>, HttpClientError>;
}

/// An implementation of the `HttpClient` trait using the ureq library.
pub struct HttpClientUreq {
    client: ureq::Agent,
    url: String,
}

impl HttpClientUreq {
    fn new(url: String, timeout: Duration) -> Self {
        Self {
            client: ureq::AgentBuilder::new().timeout(timeout).build(),
            url,
        }
    }
}

impl HttpClient for HttpClientUreq {
    fn get(&self) -> Result<http::Response<Vec<u8>>, HttpClientError> {
        Ok(self
            .client
            .get(&self.url)
            .call()
            .map_err(|e| HttpClientError::UreqError(e.to_string()))?
            .into())
    }
}

impl<C> Detect for AWSDetector<C>
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
    use http::Response;
    use mockall::mock;

    mock! {
        pub HttpClientMock {}
        impl HttpClient for HttpClientMock {
            fn get(&self) -> Result<Response<Vec<u8>>, HttpClientError>;
        }
    }

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
}
