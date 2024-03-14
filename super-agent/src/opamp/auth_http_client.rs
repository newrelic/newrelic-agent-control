use crate::auth::token::TokenRetriever;
use crate::auth::ClientID;
use http::{HeaderMap, Response};
use opamp_client::http::http_client::HttpClient;
use opamp_client::http::HttpClientError::UreqError;
use opamp_client::http::{HttpClientError, HttpClientUreq};
use std::sync::Arc;

pub struct AuthHttpClient<T>
where
    T: TokenRetriever,
{
    http_client: HttpClientUreq,
    token_retriever: Arc<T>,
}

impl<T> AuthHttpClient<T>
where
    T: TokenRetriever,
{
    pub fn new(http_client: HttpClientUreq, token_retriever: Arc<T>) -> AuthHttpClient<T> {
        Self {
            http_client,
            token_retriever,
        }
    }
}

impl<T> HttpClient for AuthHttpClient<T>
where
    T: TokenRetriever,
{
    fn post(
        &self,
        body: Vec<u8>,
        extra_headers: HeaderMap,
    ) -> Result<Response<Vec<u8>>, HttpClientError> {
        let token = self
            .token_retriever
            .retrieve()
            .map_err(|e| UreqError(format!("cannot retrieve auth token: {}", e.to_string())))?;

        let mut headers = extra_headers.clone();
        headers.append(
            "Authorization",
            format!("Bearer {}", token.access_token()).parse().unwrap(),
        );

        self.http_client.post(body, headers)
    }
}
