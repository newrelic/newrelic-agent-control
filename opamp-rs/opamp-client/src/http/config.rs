//! # Common HTTP Configuration.

use http::header::{InvalidHeaderName, InvalidHeaderValue};
use http::{HeaderMap, HeaderName, HeaderValue};
use std::str::FromStr;
use std::time::Duration;
use url::{ParseError, Url};

/// Default client timeout is 30 seconds
const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

/// A struct representing the configuration for the internal HTTP client.
///
/// # Examples
///
/// Creating a new `HttpConfig` with a URL and default headers:
///
/// ```rust
/// use opamp_client::http::HttpConfig;
///
/// let config = HttpConfig::new("https://my-server.com").unwrap();
/// ```
///
/// Adding custom headers to the configuration:
///
/// ```rust
/// use opamp_client::http::HttpConfig;
///
/// let config = HttpConfig::new("https://my-server.com").unwrap();
/// let config = config.with_headers(vec![("Authorization", "Bearer <token>")]).unwrap();
/// ```
///
/// Enabling GZIP compression in the configuration:
///
/// ```rust
/// use opamp_client::http::HttpConfig;
///
/// let config = HttpConfig::new("https://my-server.com").unwrap();
/// let config = config.with_gzip_compression(true);
/// ```
///
/// Setting a custom timeout in the configuration:
///
/// ```rust
/// use std::time::Duration;
/// use opamp_client::http::HttpConfig;
///
/// let config = HttpConfig::new("https://my-server.com").unwrap();
/// let config = config.with_timeout(Duration::from_secs(5));
/// ```
pub struct HttpConfig {
    pub(super) url: Url,
    pub(super) headers: HeaderMap,
    pub(super) compression: bool,
    pub(super) timeout: Duration,
}

/// An enumeration of potential errors related to the HTTP client.
#[derive(thiserror::Error, Debug)]
pub enum HttpConfigError {
    /// HTTP client with an invalid url.
    #[error("`{0}`")]
    InvalidUrl(#[from] ParseError),
    /// HTTP client with an invalid header value.
    #[error("`{0}`")]
    InvalidHeader(#[from] InvalidHeaderValue),
    /// HTTP client with an invalid header name.
    #[error("`{0}`")]
    InvalidHeaderName(#[from] InvalidHeaderName),
}

impl HttpConfig {
    /// Construct a new `HttpConfig` with a given URL as a string.
    ///
    /// # Errors
    ///
    /// This function will return an error if the provided URL is not valid.
    pub fn new(url: &str) -> Result<Self, HttpConfigError> {
        Ok(Self {
            url: Url::from_str(url)?,
            headers: opamp_headers(),
            compression: false,
            timeout: DEFAULT_CLIENT_TIMEOUT,
        })
    }

    /// Optionally include custom headers into the HTTP requests.
    ///
    /// Custom headers can be added using an input iterator that provides key-value pairs.
    ///
    /// If the key already exists in the current header map, the new value will overwrite the old one.
    ///
    /// # Errors
    ///
    /// This function will return an error if the provided key or value is not valid.
    pub fn with_headers<I, K, V>(mut self, headers: I) -> Result<Self, HttpConfigError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        for (ref key, ref val) in headers {
            // do nothing if value already in internal headers map
            let _ = self
                .headers
                .insert(HeaderName::from_str(key.as_ref())?, val.as_ref().parse()?);
        }
        Ok(self)
    }

    /// Enable or disable gzip compression for the HTTP requests.
    pub fn with_gzip_compression(mut self, compression: bool) -> Self {
        if compression {
            self.headers
                .insert("Content-Encoding", HeaderValue::from_static("gzip"));
            self.headers
                .insert("Accept-Encoding", HeaderValue::from_static("gzip"));
            self.compression = true;
        }
        self
    }

    /// Define a custom timeout for the http client.
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }
}

/// Returns a HeaderMap pre-populated with common HTTP headers used in an OpAMP connection
fn opamp_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();

    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/x-protobuf"),
    );

    headers
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn client_gzip_headers() {
        let http_config = HttpConfig::new("http://example.com")
            .unwrap()
            .with_gzip_compression(true);

        assert_eq!(
            http_config.headers.get("Content-Encoding"),
            Some(&HeaderValue::from_static("gzip"))
        );
        assert_eq!(
            http_config.headers.get("Accept-Encoding"),
            Some(&HeaderValue::from_static("gzip"))
        )
    }
}
