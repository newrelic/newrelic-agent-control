//! # Synchronous HTTP Client Module
use std::io::Cursor;

use http::{HeaderMap, HeaderName, Response};
use opamp_client::http::http_client::HttpClient;
use opamp_client::http::HttpClientError;
use opamp_client::http::HttpClientError::TransportError;
use ureq::Request;
use url::Url;

use crate::opamp::http::client::HttpClientUreqError::HeadersError;
use nr_auth::TokenRetriever;

#[derive(thiserror::Error, Debug)]
pub enum HttpClientUreqError {
    #[error("errors happened creating request: `{0}`")]
    RequestError(String),
    #[error("errors happened creating headers: `{0}`")]
    HeadersError(String),
}

/// An implementation of the `HttpClient` trait using the ureq library.
pub struct HttpClientUreq<T> {
    client: ureq::Agent,
    url: Url,
    headers: HeaderMap,
    token_retriever: T,
}

impl<T> HttpClientUreq<T>
where
    T: TokenRetriever,
{
    pub(super) fn new(
        client: ureq::Agent,
        url: Url,
        headers: HeaderMap,
        token_retriever: T,
    ) -> Self {
        Self {
            client,
            url,
            headers,
            token_retriever,
        }
    }

    /// headers will return the "static" headers that are added to the client in
    /// creation time + the authorization header retrieved byt the TokenRetriever
    fn headers(&self) -> Result<HeaderMap, HttpClientUreqError> {
        let mut headers = self.headers.clone();

        // Get authorization token from the token retriever
        let token = self
            .token_retriever
            .retrieve()
            .map_err(|e| HeadersError(format!("cannot retrieve auth header: {}", e)))?;

        // Insert auth token header
        headers.insert(
            HeaderName::from_static("authorization"),
            format!("Bearer {}", token.access_token()).parse().unwrap(),
        );

        Ok(headers)
    }

    /// create the HTTP Request and add the headers to it
    fn build_request(&self, headers: &HeaderMap) -> Request {
        let req = self.client.post(self.url.as_ref());

        // Add all headers to the request, omitting invalid values
        headers.iter().fold(req, |r, (key, val)| {
            let Ok(value) = val.to_str() else {
                tracing::error!("invalid header value string: {:?}, skipping", val);
                return r;
            };
            r.set(key.as_str(), value)
        })
    }
}

impl<T> HttpClient for HttpClientUreq<T>
where
    T: TokenRetriever,
{
    fn post(&self, body: Vec<u8>) -> Result<Response<Vec<u8>>, HttpClientError> {
        let headers = self.headers().map_err(|e| TransportError(e.to_string()))?;
        let request = self.build_request(&headers);

        match request.send(Cursor::new(body)) {
            Ok(response) | Err(ureq::Error::Status(_, response)) => build_response(response),
            Err(ureq::Error::Transport(e)) => {
                Err(TransportError(format!("error sending request: {}", e)))
            }
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
    use nr_auth::TokenRetrieverError;

    use crate::opamp::http::auth_token_retriever::test::{token_stub, MockTokenRetrieverMock};
    use crate::opamp::http::builder::build_ureq_client;

    use super::*;

    impl<T> HttpClientUreq<T>
    where
        T: TokenRetriever,
    {
        pub fn additional_headers(mut self, headers: HeaderMap) -> Self {
            self.headers.extend(headers);
            self
        }
    }

    #[test]
    fn test_build_request_extra_headers() {
        let url = "http://localhost".try_into().unwrap();
        let headers = Default::default();
        let ureq_client = build_ureq_client();
        let token_retriever = MockTokenRetrieverMock::default();

        let client = HttpClientUreq::new(ureq_client, url, headers, token_retriever);

        let new_headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("new-key"),
            HeaderValue::from_static("new_value"),
        )]);

        let req = client.build_request(&new_headers);

        assert_eq!(req.header("new-key").unwrap(), "new_value");
    }

    #[test]
    fn test_build_request_extra_headers_override() {
        let url = "http://localhost".try_into().unwrap();
        let ureq_client = build_ureq_client();
        let token_retriever = MockTokenRetrieverMock::default();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("existing_value"),
        )]);

        let client = HttpClientUreq::new(ureq_client, url, headers, token_retriever);

        let new_headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("new_value"),
        )]);

        let req = client.build_request(&new_headers);

        assert_eq!(req.header("existing-key").unwrap(), "new_value");
    }

    #[test]
    fn test_build_request_extra_headers_invalid_skipped() {
        let url = "http://localhost".try_into().unwrap();
        let ureq_client = build_ureq_client();
        let token_retriever = MockTokenRetrieverMock::default();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let client = HttpClientUreq::new(ureq_client, url, headers, token_retriever);

        let new_headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("new-key"),
            HeaderValue::from_bytes(&[255]).unwrap(), // invalid ascii byte
        )]);

        let req = client.build_request(&new_headers);

        assert_eq!(req.header("new-key"), None);
    }

    #[test]
    fn test_headers_auth_token_is_added() {
        let url = "http://localhost".try_into().unwrap();
        let ureq_client = build_ureq_client();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let mut token_retriever = MockTokenRetrieverMock::default();
        let token = token_stub();
        token_retriever.should_retrieve(token.clone());

        let client = HttpClientUreq::new(ureq_client, url, headers, token_retriever);

        let headers = client.headers().unwrap();

        assert_eq!(2, headers.len());
        assert_eq!("existing_value", headers.get("existing-key").unwrap());
        assert_eq!(
            format!("Bearer {}", token.access_token()).as_str(),
            headers.get("authorization").unwrap()
        );
    }

    #[test]
    fn test_headers_auth_token_returns_error() {
        let url = "http://localhost".try_into().unwrap();
        let ureq_client = build_ureq_client();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let mut token_retriever = MockTokenRetrieverMock::default();
        token_retriever.should_return_error(TokenRetrieverError::NotDefinedYetError);

        let client = HttpClientUreq::new(ureq_client, url, headers, token_retriever);

        let headers = client.headers().unwrap_err();

        assert_eq!(
            "errors happened creating headers: `cannot retrieve auth header: not defined yet`",
            headers.to_string()
        );
    }

    #[test]
    fn error_in_headers_should_be_bubbled_on_post() {
        let url = "http://localhost".try_into().unwrap();
        let ureq_client = build_ureq_client();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let mut token_retriever = MockTokenRetrieverMock::default();
        token_retriever.should_return_error(TokenRetrieverError::NotDefinedYetError);

        let client = HttpClientUreq::new(ureq_client, url, headers, token_retriever);

        let res = client.post("test".into()).unwrap_err();
        assert_eq!(
            "`errors happened creating headers: `cannot retrieve auth header: not defined yet``",
            res.to_string()
        )
    }
}
