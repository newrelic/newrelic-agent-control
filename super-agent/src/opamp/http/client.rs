//! # Synchronous HTTP Client Module
use http::{HeaderMap, Response};
use opamp_client::http::http_client::HttpClient;
use opamp_client::http::HttpClientError;
use std::io::Cursor;
use std::time::Duration;
use ureq::Request;
use url::Url;

use crate::super_agent::config::OpAMPClientConfig;

/// Default client timeout is 30 seconds
const DEFAULT_CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

/// An implementation of the `HttpClient` trait using the ureq library.
pub(super) struct HttpClientUreq {
    client: ureq::Agent,
    url: Url,
    headers: HeaderMap,
}

impl HttpClientUreq {
    fn build_request(&self, extra_headers: &HeaderMap) -> Request {
        let req = self.client.post(self.url.as_ref());

        let headers = self.headers.iter().chain(extra_headers.iter());

        // Add all headers to the request, omitting invalid values
        headers.fold(req, |r, (key, val)| {
            let Ok(value) = val.to_str() else {
                tracing::error!("invalid header value string: {:?}, skipping", val);
                return r;
            };
            r.set(key.as_str(), value)
        })
    }
}

/// Implement TryFrom trait to create a ureq::Agent from HttpConfig
impl From<&OpAMPClientConfig> for HttpClientUreq {
    fn from(config: &OpAMPClientConfig) -> Self {
        let client = ureq::AgentBuilder::new()
            .timeout_connect(DEFAULT_CLIENT_TIMEOUT)
            .timeout(DEFAULT_CLIENT_TIMEOUT)
            .build();
        let url = config.endpoint.clone();
        let headers = config.headers.clone();
        Self {
            client,
            url,
            headers,
        }
    }
}

impl HttpClient for HttpClientUreq {
    fn post(&self, body: Vec<u8>) -> Result<Response<Vec<u8>>, HttpClientError> {
        // The idea here is that we can add additional headers to the request for authentication
        // with New Relic. These headers will be retrieved from a call to the appropriate auth
        // service. So we assume we can just merge them with the existing headers.
        let auth_headers = HeaderMap::new(); // CHANGEME

        let req = self.build_request(&auth_headers);

        match req.send(Cursor::new(body)) {
            Ok(response) | Err(ureq::Error::Status(_, response)) => build_response(response),
            Err(ureq::Error::Transport(e)) => Err(HttpClientError::UreqError(e.to_string())),
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
    response.into_reader().read_to_end(&mut buf)?;

    Ok(response_builder.body(buf)?)
}

#[cfg(test)]
pub(crate) mod test {
    use http::{HeaderName, HeaderValue};

    use super::*;

    impl HttpClientUreq {
        pub fn additional_headers(mut self, headers: HeaderMap) -> Self {
            self.headers.extend(headers);
            self
        }
    }

    #[test]
    fn test_build_request_extra_headers() {
        let config = OpAMPClientConfig {
            endpoint: "http://localhost".try_into().unwrap(),
            headers: Default::default(),
        };
        let client = HttpClientUreq::from(&config);

        let new_headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("new-key"),
            HeaderValue::from_static("new_value"),
        )]);

        let req = client.build_request(&new_headers);

        assert_eq!(req.header("new-key").unwrap(), "new_value");
    }

    #[test]
    fn test_build_request_extra_headers_override() {
        let config = OpAMPClientConfig {
            endpoint: "http://localhost".try_into().unwrap(),
            headers: Default::default(),
        };
        let existing_headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let client = HttpClientUreq::from(&config).additional_headers(existing_headers);
        let new_headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("new_value"),
        )]);

        let req = client.build_request(&new_headers);

        assert_eq!(req.header("existing-key").unwrap(), "new_value");
    }

    #[test]
    fn test_build_request_extra_headers_invalid_skipped() {
        let config = OpAMPClientConfig {
            endpoint: "http://localhost".try_into().unwrap(),
            headers: Default::default(),
        };
        let existing_headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let client = HttpClientUreq::from(&config).additional_headers(existing_headers);

        let new_headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("new-key"),
            HeaderValue::from_bytes(&[255]).unwrap(), // invalid ascii byte
        )]);

        let req = client.build_request(&new_headers);

        assert_eq!(req.header("new-key"), None);
    }
}
