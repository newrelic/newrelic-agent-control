use http::HeaderMap;
use std::time::Duration;
use thiserror::Error;
use tracing::error;

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum HttpClientError {
    /// Represents Ureq crate error.
    #[error("internal HTTP client error: `{0}`")]
    UreqError(String),
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
    header_map: Option<HeaderMap>,
}

impl HttpClientUreq {
    /// Returns a new instance of HttpClientUreq
    pub fn new(url: String, timeout: Duration, header_map: Option<HeaderMap>) -> Self {
        Self {
            client: ureq::AgentBuilder::new().timeout(timeout).build(),
            url,
            header_map,
        }
    }
}

impl HttpClient for HttpClientUreq {
    fn get(&self) -> Result<http::Response<Vec<u8>>, HttpClientError> {
        let mut req = self.client.get(&self.url);

        if let Some(headers) = self.header_map.as_ref() {
            for (header_name, header_value) in headers {
                if let Ok(value) = header_value.to_str() {
                    req = req.set(header_name.as_str(), value);
                } else {
                    error!("invalid header value for {}", header_name)
                }
            }
        }

        Ok(req
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

        pub fn should_not_get(&mut self, error: HttpClientError) {
            self.expect_get().once().return_once(move || Err(error));
        }
    }
}
