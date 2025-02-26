use http::HeaderMap;
use reqwest::blocking::{Client, Response};
use std::time::Duration;
use thiserror::Error;
use tracing::error;

/// The default timeout for the HTTP client.
pub const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum HttpClientError {
    /// Represents an error building the HttpClient
    #[error("could not build the HTTP client: `{0}`")]
    BuildingError(String),
    /// Represents an internal HTTP client error.
    #[error("internal HTTP client error: `{0}`")]
    InternalError(String),
    /// Represents HTTP Transport error.
    #[error("transport HTTP client error: `{0}`")]
    TransportError(String),
    /// Represents an error in the HTTP response.
    #[error("status code: `{0}`, Reason: `{1}`")]
    ResponseError(u16, String),
}

/// The `HttpClient` trait defines the HTTP get interface to be implemented
/// by HTTP clients.
pub trait HttpClient {
    /// Returns a `http::Response<Vec<u8>>` structure as the HTTP response or
    /// HttpClientError if an error was found.
    fn get(&self) -> Result<http::Response<Vec<u8>>, HttpClientError>;
}

/// An implementation of the [HttpClient] trait using the reqwest library.
pub struct HttpClientReqwest {
    client: Client,
    url: String,
    headers: Option<HeaderMap>,
}

impl HttpClientReqwest {
    /// Builds a new [HttpClientReqwest] instance
    pub fn try_new(
        client: Client,
        url: String,
        headers: Option<HeaderMap>,
    ) -> Result<Self, HttpClientError> {
        Ok(Self {
            client,
            url,
            headers,
        })
    }
}

impl HttpClient for HttpClientReqwest {
    fn get(&self) -> Result<http::Response<Vec<u8>>, HttpClientError> {
        let mut req = self.client.get(&self.url);
        if let Some(headers) = self.headers.as_ref() {
            req = req.headers(headers.clone());
        }
        try_build_response(req.send()?)
    }
}

/// Helper to build a [http::Response<Vec<u8>>] from a reqwest's blocking response.
/// It includes status, version and body. Headers are not included but could be added if needed.
pub fn try_build_response(res: Response) -> Result<http::Response<Vec<u8>>, HttpClientError> {
    let status = res.status();
    let version = res.version();
    let body: Vec<u8> = res
        .bytes()
        .map_err(|err| HttpClientError::TransportError(err.to_string()))?
        .into();
    http::Response::builder()
        .status(status)
        .version(version)
        .body(body)
        .map_err(|err| HttpClientError::TransportError(err.to_string()))
}

impl From<reqwest::Error> for HttpClientError {
    fn from(err: reqwest::Error) -> Self {
        Self::TransportError(err.to_string())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use http::Response;
    use mockall::mock;

    mock! {
        pub HttpClientMock {}
        impl HttpClient for HttpClientMock {
            fn get(&self) -> Result<Response<Vec<u8>>, HttpClientError>;
        }
    }

    impl MockHttpClientMock {
        pub fn should_get(&mut self, response: Response<Vec<u8>>) {
            self.expect_get().once().return_once(move || Ok(response));
        }

        pub fn should_not_get(&mut self, error: HttpClientError) {
            self.expect_get().once().return_once(move || Err(error));
        }
    }
}
