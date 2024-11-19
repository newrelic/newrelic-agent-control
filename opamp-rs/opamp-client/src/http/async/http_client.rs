//! # HTTP Client Module
//!
//! This module defines an HTTP client for a Rust application, its configuration, and common errors. GZIP compression is enabled by default.
//!

use super::super::HttpConfig;
use crate::common::compression::{Compressor, CompressorError, DecoderError, EncoderError};
use async_trait::async_trait;
use reqwest::Response;
use thiserror::Error;
use url::Url;

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum AsyncHttpClientError {
    /// Represents Reqwest crate error.
    #[error("`{0}`")]
    ReqwestError(#[from] reqwest::Error),
    /// Represents a decode error.
    #[error("`{0}`")]
    DecoderError(#[from] DecoderError),
    /// Represents an encode error.
    #[error("`{0}`")]
    EncoderError(#[from] EncoderError),
    /// Represents a compression error.
    #[error("`{0}`")]
    CompressionError(#[from] CompressorError),
    /// Unsuccessful HTTP response.
    #[error("Status code: `{0}` Canonical reason: `{1}`")]
    UnsuccessfulResponse(u16, String),
}

/// An asynchronous trait that defines the internal methods for HTTP clients.
#[async_trait]
pub trait AsyncHttpClient {
    /// An asynchronous function that defines the `post` method for HTTP client.
    async fn post(&self, body: Vec<u8>) -> Result<Response, AsyncHttpClientError>;
}

/// An implementation of the `HttpClient` trait using the reqwest library.
pub struct HttpClientReqwest {
    client: reqwest::Client,
    url: Url,
}

impl HttpClientReqwest {
    /// Construct a new `HttpClientReqwest` from the given `HttpConfig`.
    ///
    /// # Examples
    ///
    /// ```
    /// use opamp_client::http::{HttpClientReqwest, HttpConfig};
    ///
    /// let config = HttpConfig::new("https://my-server.com/v1/opamp").unwrap();
    /// let client = HttpClientReqwest::new(config).unwrap();
    /// ```
    pub fn new(config: HttpConfig) -> Result<Self, AsyncHttpClientError> {
        let url = config.url.clone();
        Ok(Self {
            client: reqwest::Client::try_from(config)?,
            url,
        })
    }
}

#[async_trait]
impl AsyncHttpClient for HttpClientReqwest {
    async fn post(&self, body: Vec<u8>) -> Result<Response, AsyncHttpClientError> {
        Ok(self.client.post(self.url.clone()).body(body).send().await?)
    }
}

/// Implement TryFrom trait to create a reqwest::Client from HttpConfig
impl TryFrom<HttpConfig> for reqwest::Client {
    type Error = AsyncHttpClientError;
    fn try_from(value: HttpConfig) -> Result<Self, Self::Error> {
        Ok(reqwest::Client::builder()
            .default_headers(value.headers)
            .connect_timeout(value.timeout)
            .timeout(value.timeout)
            .build()?)
    }
}

/// Implement From trait to create a Compressor from a reference to HttpConfig
impl From<&HttpConfig> for Compressor {
    fn from(value: &HttpConfig) -> Self {
        if value.compression {
            return Compressor::Gzip;
        }
        Compressor::Plain
    }
}

#[cfg(test)]
pub(crate) mod test {
    use std::collections::HashMap;

    use http::{response::Builder, StatusCode};
    use mockall::mock;
    use reqwest::RequestBuilder;

    use prost::Message;

    use super::*;

    /////////////////////////////////////////////
    // Test helpers & mocks
    /////////////////////////////////////////////

    // Define a struct to represent the mock client
    mock! {
      pub(crate) HttpClientMockall {}

        #[async_trait]
        impl AsyncHttpClient for HttpClientMockall {
            async fn post(&self, body: Vec<u8>) -> Result<Response, AsyncHttpClientError>;
        }
    }

    impl MockHttpClientMockall {
        pub(crate) fn should_post(&mut self, reqwest_response: Response) {
            self.expect_post()
                .once()
                .return_once(move |_| Ok(reqwest_response));
        }

        #[allow(dead_code)]
        pub(crate) fn should_not_post(&mut self, error: AsyncHttpClientError) {
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

    // Create a reqwest response from a ServerToAgent
    pub(crate) fn reqwest_response_from_server_to_agent(
        server_to_agent: &crate::opamp::proto::ServerToAgent,
        response_parts: ResponseParts,
    ) -> Response {
        let mut buf = vec![];
        let _ = &server_to_agent.encode(&mut buf);

        let mut response_builder = Builder::new();
        for (k, v) in response_parts.headers {
            response_builder = response_builder.header(k, v);
        }

        let response = response_builder
            .status(response_parts.status)
            .body(buf)
            .unwrap();

        Response::from(response)
    }

    #[allow(dead_code)]
    // Create a ServerToAgent struct from a request body
    pub(crate) fn agent_to_server_from_req(
        req_builder: &RequestBuilder,
    ) -> crate::opamp::proto::AgentToServer {
        let request = req_builder.try_clone().unwrap().build().unwrap();
        let body = request.body().unwrap().as_bytes().unwrap();
        crate::opamp::proto::AgentToServer::decode(body).unwrap()
    }
}
