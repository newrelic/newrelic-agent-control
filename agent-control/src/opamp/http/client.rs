//! # Synchronous OpAMP HTTP Client
use crate::http::reqwest::try_build_response;
use crate::opamp::http::client::OpAMPHttpClientError::AuthorizationHeadersError;
use http::header::AUTHORIZATION;
use http::{HeaderMap, HeaderValue, Response};
use nr_auth::TokenRetriever;
use opamp_client::http::http_client::HttpClient;
use opamp_client::http::HttpClientError;
use reqwest::blocking::Client;
use std::sync::Arc;
use url::Url;

#[derive(thiserror::Error, Debug)]
pub enum OpAMPHttpClientError {
    #[error("could not build auth headers: `{0}`")]
    AuthorizationHeadersError(String),
}

pub struct ReqwestOpAMPClient<T: TokenRetriever> {
    client: Client,
    url: Url,
    headers: HeaderMap,
    token_retriever: Arc<T>,
}

impl<T> ReqwestOpAMPClient<T>
where
    T: TokenRetriever + Send + Sync + 'static,
{
    pub(super) fn new(
        client: Client,
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

    /// Helper to build the headers to perform each request, including the authorization header built with the value
    /// retrieved by the token retriever.
    fn headers(&self) -> Result<HeaderMap, OpAMPHttpClientError> {
        let mut headers = self.headers.clone();

        // Get authorization token from the token retriever
        let token = self.token_retriever.retrieve().map_err(|e| {
            AuthorizationHeadersError(format!("cannot retrieve auth header: {}", e))
        })?;

        // Insert auth token header
        if !token.access_token().is_empty() {
            let access_token: String = token.access_token().parse().map_err(|e| {
                AuthorizationHeadersError(format!("unable to parse the authorization token: {}", e))
            })?;
            let auth_header_string = format!("Bearer {access_token}");
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

        Ok(headers)
    }
}

impl<T> HttpClient for ReqwestOpAMPClient<T>
where
    T: TokenRetriever + Send + Sync + 'static,
{
    fn post(&self, body: Vec<u8>) -> Result<Response<Vec<u8>>, HttpClientError> {
        let headers = self.headers()?;

        let req = self
            .client
            .post(self.url.as_ref())
            .headers(headers)
            .body(body);

        let res = req
            .send()
            .map_err(|err| HttpClientError::TransportError(err.to_string()))?;

        Ok(try_build_response(res)?)
    }
}

impl From<OpAMPHttpClientError> for HttpClientError {
    fn from(err: OpAMPHttpClientError) -> Self {
        Self::TransportError(err.to_string())
    }
}

#[cfg(test)]
pub mod tests {
    use std::time::Duration;

    use assert_matches::assert_matches;
    use http::{HeaderName, HeaderValue};
    use httpmock::Method::POST;
    use httpmock::MockServer;

    use super::*;

    use chrono::Utc;
    use fake::faker::lorem::en::Word;
    use fake::Fake;
    use mockall::mock;

    use crate::http::config::HttpConfig;
    use crate::http::reqwest::try_build_reqwest_client;
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
    fn test_headers_auth_token_is_added() {
        let url = "http://localhost".try_into().unwrap();
        let http_config = HttpConfig::default();
        let reqwest_client = try_build_reqwest_client(http_config).unwrap();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let mut token_retriever = MockTokenRetrieverMock::default();
        let token = token_stub();
        token_retriever.should_retrieve(token.clone());

        let client =
            ReqwestOpAMPClient::new(reqwest_client, url, headers, Arc::new(token_retriever));

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
        let reqwest_client = try_build_reqwest_client(http_config).unwrap();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let mut token_retriever = MockTokenRetrieverMock::default();
        token_retriever
            .should_return_error(TokenRetrieverError::TokenRetrieverError("error".into()));

        let client =
            ReqwestOpAMPClient::new(reqwest_client, url, headers, Arc::new(token_retriever));

        let headers_err = client.headers().unwrap_err();
        assert_matches!(headers_err, AuthorizationHeadersError(_));
    }

    #[test]
    fn test_error_in_headers_should_be_bubbled_on_post() {
        let url = "http://localhost".try_into().unwrap();
        let http_config = HttpConfig::default();
        let reqwest_client = try_build_reqwest_client(http_config).unwrap();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);

        let mut token_retriever = MockTokenRetrieverMock::default();
        token_retriever
            .should_return_error(TokenRetrieverError::TokenRetrieverError("error".into()));

        let client =
            ReqwestOpAMPClient::new(reqwest_client, url, headers, Arc::new(token_retriever));

        let err = client.post("test".into()).unwrap_err();
        assert_matches!(err, HttpClientError::TransportError(_));
    }

    // This test seems to be testing the reqwest library but it is useful to detect particular behaviors of the
    // underlying libraries. Context: some libraries, such as ureq, return an error if any response has a status code
    // not in the 2XX range and the client implementation needs to handle that properly.
    #[test]
    fn test_http_client() {
        struct TestCase {
            name: &'static str,
            status_code: u16,
        }

        impl TestCase {
            fn run(self) {
                // Expect a request with the corresponding authorization header
                let token = token_stub();
                let path = "/";
                let mock_server = MockServer::start();
                let req_mock = mock_server.mock(|when, then| {
                    when.path(path).method(POST).header(
                        "authorization",
                        format!("Bearer {}", token.clone().access_token().as_str()),
                    );
                    then.status(self.status_code).body(self.name);
                });

                let http_config = HttpConfig::new(
                    Duration::from_secs(3),
                    Duration::from_secs(3),
                    Default::default(),
                );
                let reqwest_client = try_build_reqwest_client(http_config).unwrap_or_else(|err| {
                    panic!(
                        "unexpected error building the reqwest client {} - {}",
                        err, self.name
                    )
                });
                let mut token_retriever = MockTokenRetrieverMock::default();
                token_retriever.should_retrieve(token.clone());
                let url: Url = mock_server.url(path).parse().unwrap_or_else(|err| {
                    panic!(
                        "could not parse the mock-server url: {} - {}",
                        err, self.name
                    )
                });

                let client = ReqwestOpAMPClient::new(
                    reqwest_client,
                    url,
                    HeaderMap::default(),
                    Arc::new(token_retriever),
                );

                let res = client.post("some-request-body".into()).unwrap();

                req_mock.assert_calls(1);
                assert_eq!(
                    res.status(),
                    self.status_code,
                    "not expected status code in {}",
                    self.name
                );
                assert_eq!(
                    *res.body(),
                    self.name.to_string().as_bytes().to_vec(),
                    "not expected body code in {}",
                    self.name
                );
            }
        }
        let test_cases = [
            TestCase {
                name: "OK",
                status_code: 200,
            },
            TestCase {
                name: "Not found",
                status_code: 404,
            },
            TestCase {
                name: "Server error",
                status_code: 500,
            },
        ];
        test_cases.into_iter().for_each(|tc| tc.run());
    }
}
