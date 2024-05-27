use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

use crate::{token::AccessToken, ClientID};

#[derive(Error, Debug)]
pub enum AuthenticateError {
    #[error("unable to deserialize token: `{0}`")]
    DeserializeError(String),
    #[error("identity server error: Status code: `{0}`, response `{1}`")]
    HttpResponseError(u16, String),
    #[error("http transport error: `{0}`")]
    HttpTransportError(String),
}

impl From<ureq::Error> for AuthenticateError {
    fn from(value: ureq::Error) -> Self {
        match value {
            ureq::Error::Status(code, resp) => {
                AuthenticateError::HttpResponseError(code, resp.into_string().unwrap_or_default())
            }
            ureq::Error::Transport(e) => AuthenticateError::HttpTransportError(e.to_string()),
        }
    }
}

/// The Authenticator is responsible for obtaining a valid JWT token from System Identity Service.
pub struct Authenticator {
    http_client: ureq::Agent,
    url: String,
}

/// Authenticator configuration
pub struct AuthenticatorConfig {
    /// System Identity Service URL
    pub url: String,
    /// HTTP client connection and request timeout
    pub timeout: Duration,
}

impl Authenticator {
    pub fn new(config: AuthenticatorConfig) -> Self {
        Self {
            http_client: ureq::AgentBuilder::new()
                .timeout_connect(config.timeout)
                .timeout(config.timeout)
                .build(),
            url: config.url,
        }
    }

    /// Executes a POST request to Authentication Server with the `Request` as a body and returns a `Response`.
    pub fn authenticate(&self, req: Request) -> Result<Response, AuthenticateError> {
        let encoded_response = self.http_client.post(&self.url).send_json(req)?;

        let response: Response = encoded_response
            .into_json()
            .map_err(|e| AuthenticateError::DeserializeError(e.to_string()))?;

        Ok(response)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantType {
    ClientCredentials,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientAssertionType {
    #[serde(rename(serialize = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"))]
    JwtBearer,
}

type ClientAssertion = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub client_id: ClientID,
    pub grant_type: GrantType,
    pub client_assertion_type: ClientAssertionType,
    pub client_assertion: ClientAssertion,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Response {
    pub access_token: AccessToken,
    pub expires_in: u32,
    pub token_type: String,
}

#[cfg(test)]
mod test {
    use assert_matches::assert_matches;
    use std::time::Duration;

    use httpmock::{Method::POST, MockServer};

    use crate::authenticator::AuthenticateError;

    use super::{
        Authenticator, ClientAssertion, ClientAssertionType, ClientID, GrantType, Request, Response,
    };

    #[test]
    fn authentication_succeed() {
        let (request, expected_response) = fake_request_response();

        let identity_server_path = "/v1/authorize";
        let identity_server = MockServer::start();
        let mock = identity_server.mock(|when, then| {
            when.method(POST)
                .path(identity_server_path)
                .json_body(serde_json::to_value(request.clone()).unwrap());
            then.status(200)
                .json_body(serde_json::to_value(expected_response.clone()).unwrap());
        });

        let authenticator = Authenticator::new(super::AuthenticatorConfig {
            url: identity_server.url(identity_server_path),
            timeout: Duration::from_millis(100),
        });

        let response = authenticator.authenticate(request).unwrap();

        assert_eq!(response, expected_response);
        mock.assert()
    }

    #[test]
    fn authentication_timeout() {
        let (request, _) = fake_request_response();
        let timeout = Duration::from_millis(10);

        let identity_server_path = "/v1/authorize";
        let identity_server = MockServer::start();
        let mock = identity_server.mock(|when, then| {
            when.method(POST).path(identity_server_path);
            then.status(200)
                .delay(timeout.saturating_add(Duration::from_millis(1)));
        });

        let authenticator = Authenticator::new(super::AuthenticatorConfig {
            url: identity_server.url(identity_server_path),
            timeout,
        });

        let error = authenticator.authenticate(request).unwrap_err();

        assert!(error.to_string().to_ascii_lowercase().contains("timed out"));
        assert_matches!(error, AuthenticateError::HttpTransportError(_));
        mock.assert()
    }

    #[test]
    fn authentication_deserialize_error() {
        let (request, _) = fake_request_response();

        let identity_server_path = "/v1/authorize";
        let identity_server = MockServer::start();
        let mock = identity_server.mock(|when, then| {
            when.method(POST).path(identity_server_path);
            then.status(200)
                .body("this body should fail to be deserialized as Response");
        });

        let authenticator = Authenticator::new(super::AuthenticatorConfig {
            url: identity_server.url(identity_server_path),
            timeout: Duration::from_millis(100),
        });

        let error = authenticator.authenticate(request).unwrap_err();

        assert_matches!(error, AuthenticateError::DeserializeError(_));
        mock.assert()
    }

    #[test]
    fn authentication_server_response_error() {
        let (request, _) = fake_request_response();

        let identity_server_path = "/v1/authorize";
        let identity_server = MockServer::start();
        let mock = identity_server.mock(|when, then| {
            when.method(POST).path(identity_server_path);
            then.status(401);
        });

        let authenticator = Authenticator::new(super::AuthenticatorConfig {
            url: identity_server.url(identity_server_path),
            timeout: Duration::from_millis(100),
        });

        let error = authenticator.authenticate(request).unwrap_err();

        assert_matches!(error, AuthenticateError::HttpResponseError(401, _));
        mock.assert()
    }

    fn fake_request_response() -> (Request, Response) {
        (
            Request {
                client_id: ClientID::from("fake_id"),
                grant_type: GrantType::ClientCredentials,
                client_assertion_type: ClientAssertionType::JwtBearer,
                client_assertion: ClientAssertion::from("fake_assertion"),
            },
            Response {
                access_token: "fake_token".to_string(),
                token_type: "fake_token_type".to_string(),
                expires_in: 10,
            },
        )
    }
}
