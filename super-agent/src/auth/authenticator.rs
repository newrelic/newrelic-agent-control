use crate::auth::http_client::{HttpClient, HttpClientError};
use crate::auth::{AccessToken, ClientAssertion, ClientID};
use serde::{Deserialize, Serialize};
use std::str::Utf8Error;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuthenticateError {
    #[error("unable to authenticate token: `{0}`")]
    AuthenticateError(String),
    #[error("unable to deserialize token: `{0}`")]
    DeserializeError(String),
    #[error("http client error: `{0}`")]
    HttpClientError(#[from] HttpClientError),
    #[error("utf8 error: `{0}`")]
    Utf8ClientError(#[from] Utf8Error),
}

/// Authenticator will receive an oauth request and it will return a response with a valid JWT token
/// POST /oauth/token
///
/// Response:
/// {
///    "access_token": "<JWT>", // see Token
///    "expires_in": 3600,
///    "token_type": "Bearer"
/// }   
pub trait Authenticator {
    fn authenticate(&self, req: Request) -> Result<Response, AuthenticateError>;
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantType {
    ClientCredentials,
}

#[derive(Debug, Serialize)]
pub enum ClientAssertionType {
    #[serde(rename(serialize = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"))]
    JwtBearer,
}

#[derive(Serialize)]
pub struct Request<'a> {
    client_id: &'a ClientID,
    grant_type: GrantType,
    client_assertion_type: ClientAssertionType,
    client_assertion: ClientAssertion,
}

impl<'a> Request<'a> {
    pub fn new(
        client_id: &'a ClientID,
        grant_type: GrantType,
        client_assertion_type: ClientAssertionType,
        client_assertion: ClientAssertion,
    ) -> Self {
        Request {
            client_id,
            grant_type,
            client_assertion_type,
            client_assertion,
        }
    }
}

#[derive(Deserialize)]
pub struct Response {
    access_token: AccessToken,
    expires_in: u32,
    token_type: String,
}

impl Response {
    pub fn access_token(&self) -> &AccessToken {
        &self.access_token
    }

    pub fn expires_in_seconds(&self) -> &u32 {
        &self.expires_in
    }
}

pub struct HttpAuthenticator<C>
where
    C: HttpClient,
{
    http_client: C,
}

impl<C> HttpAuthenticator<C>
where
    C: HttpClient,
{
    pub fn new(http_client: C) -> HttpAuthenticator<C> {
        HttpAuthenticator { http_client }
    }
}

impl<C> Authenticator for HttpAuthenticator<C>
where
    C: HttpClient,
{
    fn authenticate(&self, req: Request) -> Result<Response, AuthenticateError> {
        let encoded_response = self.http_client.post(req)?;
        let response: Response =
            serde_json::from_str(std::str::from_utf8(encoded_response.body())?)
                .map_err(|e| AuthenticateError::DeserializeError(e.to_string()))?;
        Ok(response)
    }
}
