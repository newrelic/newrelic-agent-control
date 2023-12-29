use std::collections::HashMap;

use reqwest::blocking::Response;
use thiserror::Error;

use crate::{cloud::AWS_INSTANCE_ID, Detect, DetectError, Key, Resource, Value};

use super::metadata::{AWSMetadata, IPV4_METADATA_ENDPOINT};

pub struct AWSDetector<C: HttpClient> {
    http_client: C,
}

impl Default for AWSDetector<HttpClientReqwest> {
    fn default() -> Self {
        Self {
            http_client: HttpClientReqwest::new(IPV4_METADATA_ENDPOINT.to_string()),
        }
    }
}

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum HttpClientError {
    /// Represents Reqwest crate error.
    #[error("`{0}`")]
    ReqwestError(#[from] reqwest::Error),
}

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum AWSDetectorError {
    #[error("`{0}`")]
    HttpError(#[from] HttpClientError),
    #[error("error while deserializing endpoint metadata: `{0}`")]
    DeserializeError(String),
    #[error("`{0}`")]
    JsonError(#[from] serde_json::Error),
    /// Unsuccessful HTTP response.
    #[error("Status code: `{0}` Canonical reason: `{1}`")]
    UnsuccessfulResponse(u16, String),
}

pub trait HttpClient {
    fn get(&self) -> Result<Response, HttpClientError>;
}

/// An implementation of the `HttpClient` trait using the reqwest library.
pub struct HttpClientReqwest {
    client: reqwest::blocking::Client,
    url: String,
}

impl HttpClientReqwest {
    fn new(url: String) -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
            url,
        }
    }
}

impl HttpClient for HttpClientReqwest {
    fn get(&self) -> Result<Response, HttpClientError> {
        Ok(self.client.get(self.url.clone()).send()?)
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

        let metadata: AWSMetadata = serde_json::from_slice(
            &response
                .bytes()
                .map_err(|e| AWSDetectorError::DeserializeError(e.to_string()))?,
        )
        .map_err(AWSDetectorError::JsonError)?;

        Ok(Resource {
            attributes: HashMap::from([(
                Key::from(AWS_INSTANCE_ID),
                Value::from(metadata.instance_id),
            )]),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use mockall::mock;

    mock! {
        pub HttpClientMock {}
        impl HttpClient for HttpClientMock {
            fn get(&self) -> Result<Response, HttpClientError>;
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
"#,
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
