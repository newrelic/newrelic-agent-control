use http::{HeaderMap, Request, Response};
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

/// The `HttpClient` trait defines the HTTP send interface to be implemented
/// by HTTP clients.
pub trait HttpClient {
    /// Returns a `http::Response<Vec<u8>>` structure as the HTTP response or
    /// HttpClientError if an error was found.
    fn send(&self, request: Request<Vec<u8>>) -> Result<Response<Vec<u8>>, HttpClientError>;

    /// Performs a get request with the provided url and headers.
    fn get(&self, url: String, headers: HeaderMap) -> Result<Response<Vec<u8>>, HttpClientError> {
        let mut request = Request::builder()
            .method("GET")
            .uri(url)
            .body(Vec::new())
            .map_err(|e| HttpClientError::BuildingError(e.to_string()))?;
        request.headers_mut().extend(headers);
        self.send(request)
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
            fn send(&self,request: Request<Vec<u8>>) -> Result<Response<Vec<u8>>, HttpClientError>;
        }
    }

    impl MockHttpClientMock {
        pub fn should_send(&mut self, response: Response<Vec<u8>>) {
            self.expect_send().once().return_once(move |_| Ok(response));
        }
        pub fn should_not_send(&mut self, error: HttpClientError) {
            self.expect_send().once().return_once(move |_| Err(error));
        }
    }
}
