//! # Synchronous HTTP Client Module
use std::io::Cursor;
use std::sync::Arc;

use http::header::AUTHORIZATION;
use http::{HeaderMap, HeaderValue, Response};
use opamp_client::http::http_client::HttpClient;
use opamp_client::http::HttpClientError;
use opamp_client::http::HttpClientError::TransportError;
use ureq::{Agent, Request};
use url::Url;

use crate::http::ureq::build_response;
use crate::opamp::http::client::HttpClientUreqError::AuthorizationHeadersError;
use nr_auth::TokenRetriever;

#[derive(thiserror::Error, Debug)]
pub enum HttpClientUreqError {
    #[error("errors happened creating headers: `{0}`")]
    AuthorizationHeadersError(String),
}

/// An implementation of the `HttpClient` trait using the ureq library.
pub struct HttpClientUreq<T> {
    client: Agent,
    url: Url,
    headers: HeaderMap,
    token_retriever: Arc<T>,
}

impl<T> HttpClientUreq<T>
where
    T: TokenRetriever + Send + Sync + 'static,
{
    pub(super) fn new(
        client: Agent,
        url: Url,
        headers: HeaderMap,
        token_retriever: Arc<T>,
    ) -> Self {
        Self {
            client,
            url,
            headers,
            token_retriever,
        }
    }

    /// headers will return the "static" headers that are added to the client in
    /// creation time + the authorization header retrieved by the TokenRetriever
    fn headers(&self) -> Result<HeaderMap, HttpClientUreqError> {
        let mut headers = self.headers.clone();

        // Get authorization token from the token retriever
        let token = self.token_retriever.retrieve().map_err(|e| {
            AuthorizationHeadersError(format!("cannot retrieve auth header: {}", e))
        })?;

        // Insert auth token header
        if !token.access_token().is_empty() {
            let auth_header_string =
                format!("Bearer {}", token.access_token().parse::<String>().unwrap());
            let mut auth_header_value = HeaderValue::from_str(auth_header_string.as_str())
                .map_err(|e| {
                    AuthorizationHeadersError(format!(
                        "error converting '{}' to a header string: {}",
                        auth_header_string, e
                    ))
                })?;
            auth_header_value.set_sensitive(true);

            headers.insert(AUTHORIZATION, auth_header_value);
        }
        //TODO warn else case :point-up: once Token authentication is required.
        //warn!("received empty authorization token");

        Ok(headers)
    }

    /// create the HTTP Request and add the headers to it
    fn build_request(&self, headers: &HeaderMap) -> Request {
        let req = self.client.post(self.url.as_ref());

        // Add all headers to the request, omitting invalid values
        headers.iter().fold(req, |r, (key, val)| {
            // TODO: Here we are transforming HeaderValue to string and we lose control if the header
            // is sensitive or not. We are limited by `ureq` that (as of today) it only censors the
            // headers "Cookie" and "Authorization". We should change the http client to use reqwest
            // that honors the `is_sensitive` property while logging.
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
    T: TokenRetriever + Send + Sync + 'static,
{
    fn post(&self, body: Vec<u8>) -> Result<Response<Vec<u8>>, HttpClientError> {
        let headers = self.headers().map_err(|e| TransportError(e.to_string()))?;
        let request = self.build_request(&headers);

        match request.send(Cursor::new(body)) {
            Ok(response) | Err(ureq::Error::Status(_, response)) => Ok(build_response(response)
                .map_err(|e| HttpClientError::TransportError(e.to_string()))?),
            Err(ureq::Error::Transport(e)) => {
                Err(TransportError(format!("error sending request: {}", e)))
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use assert_matches::assert_matches;
    use http::{HeaderName, HeaderValue};

    use super::*;

    use chrono::Utc;
    use fake::faker::lorem::en::Word;
    use fake::Fake;
    use mockall::mock;

    use crate::http::config::HttpConfig;
    use crate::http::ureq::try_build_ureq;
    use nr_auth::token::{AccessToken, Token, TokenType};
    use nr_auth::{TokenRetriever, TokenRetrieverError};

    mock! {
        pub TokenRetrieverMock {}

        impl TokenRetriever for TokenRetrieverMock{
            fn retrieve(&self) -> Result<Token, TokenRetrieverError>;
        }
    }

    impl MockTokenRetrieverMock {
        pub fn should_retrieve(&mut self, token: Token) {
            self.expect_retrieve().once().return_once(move || Ok(token));
        }

        pub fn should_return_error(&mut self, error: TokenRetrieverError) {
            self.expect_retrieve()
                .once()
                .return_once(move || Err(error));
        }
    }

    pub fn token_stub() -> Token {
        Token::new(
            AccessToken::from(Word().fake::<String>()),
            TokenType::Bearer,
            Utc::now(),
        )
    }

    #[test]
    fn test_build_request_extra_headers() {
        let url = "http://localhost".try_into().unwrap();
        let headers = Default::default();
        let http_config = HttpConfig::default();
        let ureq_client = try_build_ureq(http_config).unwrap();
        let token_retriever = MockTokenRetrieverMock::default();

        let client = HttpClientUreq::new(ureq_client, url, headers, Arc::new(token_retriever));

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
        let http_config = HttpConfig::default();
        let ureq_client = try_build_ureq(http_config).unwrap();
        let token_retriever = MockTokenRetrieverMock::default();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("existing_value"),
        )]);

        let client = HttpClientUreq::new(ureq_client, url, headers, Arc::new(token_retriever));

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
        let http_config = HttpConfig::default();
        let ureq_client = try_build_ureq(http_config).unwrap();
        let token_retriever = MockTokenRetrieverMock::default();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let client = HttpClientUreq::new(ureq_client, url, headers, Arc::new(token_retriever));

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
        let http_config = HttpConfig::default();
        let ureq_client = try_build_ureq(http_config).unwrap();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let mut token_retriever = MockTokenRetrieverMock::default();
        let token = token_stub();
        token_retriever.should_retrieve(token.clone());

        let client = HttpClientUreq::new(ureq_client, url, headers, Arc::new(token_retriever));

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
        let http_config = HttpConfig::default();
        let ureq_client = try_build_ureq(http_config).unwrap();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let mut token_retriever = MockTokenRetrieverMock::default();
        token_retriever
            .should_return_error(TokenRetrieverError::TokenRetrieverError("error".into()));

        let client = HttpClientUreq::new(ureq_client, url, headers, Arc::new(token_retriever));

        let headers_err = client.headers().unwrap_err();
        assert_matches!(headers_err, AuthorizationHeadersError(_));
    }

    #[test]
    fn error_in_headers_should_be_bubbled_on_post() {
        let url = "http://localhost".try_into().unwrap();
        let http_config = HttpConfig::default();
        let ureq_client = try_build_ureq(http_config).unwrap();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let mut token_retriever = MockTokenRetrieverMock::default();
        token_retriever
            .should_return_error(TokenRetrieverError::TokenRetrieverError("error".into()));

        let client = HttpClientUreq::new(ureq_client, url, headers, Arc::new(token_retriever));

        let res = client.post("test".into()).unwrap_err();
        assert_eq!(
            "`errors happened creating headers: `cannot retrieve auth header: retrieving token: `error```",
            res.to_string()
        )
    }
}
