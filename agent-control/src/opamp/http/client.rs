//! # Synchronous OpAMP HTTP Client
use crate::http::client::{HttpClient, HttpResponseError};
use crate::opamp::http::client::OpAMPHttpClientError::AuthorizationHeadersError;
use http::header::AUTHORIZATION;
use http::{HeaderMap, HeaderValue, Response};
use nr_auth::TokenRetriever;
use opamp_client::http::HttpClientError;
use opamp_client::http::http_client::HttpClient as OpampHttpClient;
use std::sync::Arc;
use url::Url;

#[derive(thiserror::Error, Debug)]
pub enum OpAMPHttpClientError {
    #[error("could not build auth headers: `{0}`")]
    AuthorizationHeadersError(String),
}

pub struct HttpOpAMPClient<T: TokenRetriever> {
    client: HttpClient,
    url: Url,
    headers: HeaderMap,
    token_retriever: Arc<T>,
}

impl<T> HttpOpAMPClient<T>
where
    T: TokenRetriever + Send + Sync + 'static,
{
    pub(super) fn new(
        client: HttpClient,
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

impl<T> OpampHttpClient for HttpOpAMPClient<T>
where
    T: TokenRetriever + Send + Sync + 'static,
{
    fn post(&self, body: Vec<u8>) -> Result<Response<Vec<u8>>, HttpClientError> {
        let headers = self.headers()?;
        let mut request = http::Request::builder()
            .method("POST")
            .uri(self.url.as_str())
            .body(body)
            .map_err(|err| HttpResponseError::BuildingRequest(err.to_string()))?;
        for (key, value) in &headers {
            request.headers_mut().insert(key, value.clone());
        }
        Ok(self.client.send(request)?)
    }
}

impl From<OpAMPHttpClientError> for HttpClientError {
    fn from(err: OpAMPHttpClientError) -> Self {
        Self::TransportError(err.to_string())
    }
}

#[cfg(test)]
pub mod tests {

    use assert_matches::assert_matches;
    use http::{HeaderName, HeaderValue};

    use super::*;

    use chrono::Utc;
    use fake::Fake;
    use fake::faker::lorem::en::Word;
    use mockall::mock;

    use crate::http::config::HttpConfig;
    use nr_auth::token::{AccessToken, Token, TokenType};
    use nr_auth::{TokenRetriever, TokenRetrieverError};

    mock! {
        pub TokenRetriever {}

        impl TokenRetriever for TokenRetriever{
            fn retrieve(&self) -> Result<Token, TokenRetrieverError>;
        }
    }

    impl MockTokenRetriever {
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
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);
        let http_client = HttpClient::new(http_config).unwrap();

        let mut token_retriever = MockTokenRetriever::default();
        let token = token_stub();
        token_retriever.should_retrieve(token.clone());

        let client = HttpOpAMPClient::new(http_client, url, headers, Arc::new(token_retriever));

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
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);
        let http_client = HttpClient::new(http_config).unwrap();

        let mut token_retriever = MockTokenRetriever::default();
        token_retriever
            .should_return_error(TokenRetrieverError::TokenRetrieverError("error".into()));

        let client = HttpOpAMPClient::new(http_client, url, headers, Arc::new(token_retriever));

        let headers_err = client.headers();
        assert_matches!(headers_err, Err(AuthorizationHeadersError(_)));
    }

    #[test]
    fn test_error_in_headers_should_be_bubbled_on_post() {
        let url = "http://localhost".try_into().unwrap();
        let http_config = HttpConfig::default();
        let headers = HeaderMap::from_iter(vec![(
            HeaderName::from_static("existing-key"),
            HeaderValue::from_static("existing_value"),
        )]);
        let http_client = HttpClient::new(http_config).unwrap();

        let mut token_retriever = MockTokenRetriever::default();
        token_retriever
            .should_return_error(TokenRetrieverError::TokenRetrieverError("error".into()));

        let client = HttpOpAMPClient::new(http_client, url, headers, Arc::new(token_retriever));

        let err = client.post("test".into()).unwrap_err();
        assert_matches!(err, HttpClientError::TransportError(_));
    }
}
