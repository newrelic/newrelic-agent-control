//! # Synchronous HTTP Client Module
use std::io::Cursor;

use http::{HeaderMap, Response};
use url::Url;

use crate::common::compression::{CompressorError, DecoderError, EncoderError};
use crate::http::HttpConfig;

/// An enumeration of potential errors related to the HTTP client.
#[derive(thiserror::Error, Debug)]
pub enum HttpClientError {
    /// Represents an http transport crate error.
    #[error("`{0}`")]
    TransportError(String),
    /// Unsuccessful HTTP response.
    #[error("Status code: `{0}` Canonical reason: `{1}`")]
    UnsuccessfulResponse(u16, String),
    /// Represents a decode error.
    #[error("`{0}`")]
    DecoderError(#[from] DecoderError),
    /// Represents an encode error.
    #[error("`{0}`")]
    EncoderError(#[from] EncoderError),
    /// Represents a compression error.
    #[error("`{0}`")]
    CompressionError(#[from] CompressorError),
    /// Represents an http crate consume body error.
    #[error("`{0}`")]
    HTTPBodyError(String),
}

/// A synchronous trait that defines the internal methods for HTTP clients.
pub trait HttpClient {
    /// A synchronous function that defines the `post` method for HTTP client.
    fn post(&self, body: Vec<u8>) -> Result<Response<Vec<u8>>, HttpClientError>;
}

/// An implementation of the `HttpClient` trait using the ureq library.
pub struct HttpClientUreq {
    client: ureq::Agent,
    url: Url,
    headers: HeaderMap,
}

impl HttpClientUreq {
    /// Construct a new `HttpClientUreq` from the given `HttpConfig`.
    pub fn new(config: HttpConfig) -> Result<Self, HttpClientError> {
        let url = config.url.clone();
        let headers = config.headers.clone();
        Ok(Self {
            client: ureq::Agent::try_from(config)?,
            url,
            headers,
        })
    }
}

/// Implement TryFrom trait to create a ureq::Agent from HttpConfig
impl TryFrom<HttpConfig> for ureq::Agent {
    type Error = HttpClientError;
    /// TODO: define timeout in config
    fn try_from(value: HttpConfig) -> Result<Self, Self::Error> {
        Ok(ureq::AgentBuilder::new()
            .timeout_connect(value.timeout)
            .timeout(value.timeout)
            .build())
    }
}

impl HttpClient for HttpClientUreq {
    fn post(&self, body: Vec<u8>) -> Result<Response<Vec<u8>>, HttpClientError> {
        let mut req = self.client.post(self.url.as_str());

        for (name, value) in self.headers.iter() {
            if let Ok(value) = value.to_str() {
                req = req.set(name.as_str(), value);
            } else {
                tracing::error!("invalid header value string: {:?}", value);
            }
        }

        match req.send(Cursor::new(body)) {
            Ok(response) | Err(ureq::Error::Status(_, response)) => build_response(response),

            Err(ureq::Error::Transport(e)) => Err(HttpClientError::TransportError(e.to_string())),
        }
    }
}

fn build_response(response: ureq::Response) -> Result<Response<Vec<u8>>, HttpClientError> {
    let http_version = match response.http_version() {
        "HTTP/0.9" => http::Version::HTTP_09,
        "HTTP/1.0" => http::Version::HTTP_10,
        "HTTP/1.1" => http::Version::HTTP_11,
        "HTTP/2.0" => http::Version::HTTP_2,
        "HTTP/3.0" => http::Version::HTTP_3,
        _ => unreachable!(),
    };

    let response_builder = http::Response::builder()
        .status(response.status())
        .version(http_version);

    let mut buf: Vec<u8> = vec![];
    response
        .into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| HttpClientError::HTTPBodyError(format!("cannot read ureq response: {}", e)))?;

    response_builder
        .body(buf)
        .map_err(|e| HttpClientError::HTTPBodyError(format!("cannot build body: {}", e)))
}

#[cfg(test)]
pub(crate) mod test {
    use std::collections::HashMap;

    use http::{response::Builder, StatusCode};
    use httpmock::{Method::POST, MockServer};
    use mockall::mock;
    use prost::Message;

    use super::*;

    /////////////////////////////////////////////
    // Test
    /////////////////////////////////////////////

    #[test]
    fn test_fail_post_status_respose_error() {
        let server = MockServer::start();
        let path = "/v1/opamp";
        let expected_status_code = 401;
        let _ = server.mock(|when, then| {
            when.method(POST).path(path);
            then.status(expected_status_code);
        });

        let config = HttpConfig::new(&server.url(path)).unwrap();

        let http_client = HttpClientUreq::new(config).unwrap();

        let response = http_client
            .post("test".into())
            .expect("expect a response when fail reason contains response");
        assert_eq!(response.status(), expected_status_code);
    }

    #[test]
    fn test_fail_post_transport_error() {
        let config =
            HttpConfig::new("http://127.0.0.1:59352/no-http-server-should-listen-here").unwrap();
        let http_client = HttpClientUreq::new(config).unwrap();
        match http_client.post("test".into()) {
            Err(HttpClientError::TransportError(_)) => (),
            _ => panic!("Transport error from Ureq expected"),
        }
    }

    /////////////////////////////////////////////
    // Test helpers & mocks
    /////////////////////////////////////////////

    // Define a struct to represent the mock client
    mock! {
      pub(crate) HttpClientMockall {}

        impl HttpClient for HttpClientMockall {
            fn post(&self, body: Vec<u8>) -> Result<Response<Vec<u8>>, HttpClientError>;
        }
    }

    impl MockHttpClientMockall {
        pub(crate) fn should_post(&mut self, response: Response<Vec<u8>>) {
            self.expect_post().once().return_once(move |_| Ok(response));
        }

        #[allow(dead_code)]
        pub(crate) fn should_not_post(&mut self, error: HttpClientError) {
            self.expect_post().once().return_once(move |_| Err(error));
        }
    }

    pub(crate) struct ResponseParts {
        pub(crate) status: StatusCode,
        pub(crate) headers: HashMap<String, String>,
    }

    impl Default for ResponseParts {
        fn default() -> Self {
            ResponseParts {
                status: StatusCode::OK,
                headers: HashMap::new(),
            }
        }
    }

    // Create a ureq response from a ServerToAgent
    pub(crate) fn response_from_server_to_agent(
        server_to_agent: &crate::opamp::proto::ServerToAgent,
        response_parts: ResponseParts,
    ) -> Response<Vec<u8>> {
        let mut buf = vec![];
        let _ = &server_to_agent.encode(&mut buf);

        let mut response_builder = Builder::new();
        for (k, v) in response_parts.headers {
            response_builder = response_builder.header(k, v);
        }

        response_builder
            .status(response_parts.status)
            .body(buf)
            .unwrap()
    }
}
