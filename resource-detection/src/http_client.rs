use std::time::Duration;
use thiserror::Error;

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum HttpClientError {
    /// Represents Ureq crate error.
    #[error("internal client error: `{0}`")]
    UreqError(String),
}

/// The `HttpClient` trait defines the HTTP get interface to be implemented
/// by HTTP clients.
pub trait HttpClient {
    fn get(&self) -> Result<http::Response<Vec<u8>>, HttpClientError>;
}

/// An implementation of the `HttpClient` trait using the ureq library.
pub struct HttpClientUreq {
    client: ureq::Agent,
    url: String,
}

impl HttpClientUreq {
    pub fn new(url: String, timeout: Duration) -> Self {
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

#[cfg(test)]
pub(crate) mod test {
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
    }
}
