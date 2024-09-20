use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::http_client::HttpClient;
use crate::{token::AccessToken, ClientID};

#[derive(Error, Debug)]
pub enum AuthenticateError {
    #[error("unable to serialize request: `{0}`")]
    SerializeError(String),
    #[error("unable to deserialize token: `{0}`")]
    DeserializeError(String),
    #[error("identity server error: Status code: `{0}`, Reason: `{1}`")]
    HttpResponseError(u16, String),
    #[error("http transport error: `{0}`")]
    HttpTransportError(String),
}

impl From<ureq::Error> for AuthenticateError {
    fn from(value: ureq::Error) -> Self {
        match value {
            ureq::Error::Status(code, resp) => {
                AuthenticateError::HttpResponseError(code, resp.status_text().to_string())
            }
            ureq::Error::Transport(e) => AuthenticateError::HttpTransportError(e.to_string()),
        }
    }
}

pub trait Authenticator {
    fn authenticate(&self, req: Request) -> Result<Response, AuthenticateError>;
}

/// The Authenticator is responsible for obtaining a valid JWT token from System Identity Service.
pub struct HttpAuthenticator<C> {
    /// HTTP client
    http_client: C,
    /// System Identity Service URL
    url: Url,
}

impl<C> HttpAuthenticator<C> {
    pub fn new(http_client: C, url: Url) -> Self {
        Self { http_client, url }
    }
}

impl<C> Authenticator for HttpAuthenticator<C>
where
    C: HttpClient,
{
    /// Executes a POST request to Authentication Server with the `Request` as a body and returns a `Response`.
    fn authenticate(&self, req: Request) -> Result<Response, AuthenticateError> {
        let serialized_req = serde_json::to_string(&req)
            .map_err(|e| AuthenticateError::SerializeError(e.to_string()))?;

        let response = self
            .http_client
            .post(self.url.as_str(), serialized_req.into_bytes())
            .map_err(|e| AuthenticateError::HttpTransportError(e.to_string()))?;

        let body: String = String::from_utf8(response.body().clone()).map_err(|e| {
            AuthenticateError::DeserializeError(format!("invalid utf8 response: {}", e))
        })?;

        if !response.status().is_success() {
            return Err(AuthenticateError::HttpResponseError(
                response.status().as_u16(),
                body,
            ));
        }

        serde_json::from_str(body.as_str())
            .map_err(|e| AuthenticateError::DeserializeError(e.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantType {
    ClientCredentials,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientAssertionType {
    #[serde(rename = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer")]
    JwtBearer,
}

type ClientAssertion = String;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub client_id: ClientID,
    pub grant_type: GrantType,
    pub client_assertion_type: ClientAssertionType,
    pub client_assertion: ClientAssertion,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Response {
    pub access_token: AccessToken,
    /// The lifetime in seconds of the access token.
    pub expires_in: u32,
    pub token_type: String,
}

#[cfg(test)]
pub mod test {
    use std::time::Duration;

    use assert_matches::assert_matches;
    use httpmock::{Method::POST, MockServer};
    use mockall::mock;
    use url::Url;

    use super::{
        ClientAssertion, ClientAssertionType, ClientID, GrantType, HttpAuthenticator, Request,
        Response,
    };
    use crate::authenticator::{AuthenticateError, Authenticator};
    use crate::http_client::HttpClientUreq;

    mock! {
         pub AuthenticatorMock {}

        impl Authenticator for AuthenticatorMock
        {
            fn authenticate(&self, req: Request) -> Result<Response, AuthenticateError>;
        }
    }

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

        let timeout = Duration::from_millis(100);
        let http_client = HttpClientUreq::new(timeout);
        let authenticator = HttpAuthenticator::new(
            http_client,
            Url::parse(&identity_server.url(identity_server_path)).unwrap(),
        );

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

        let http_client = HttpClientUreq::new(timeout);
        let authenticator = HttpAuthenticator::new(
            http_client,
            Url::parse(&identity_server.url(identity_server_path)).unwrap(),
        );

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

        let timeout = Duration::from_millis(100);
        let http_client = HttpClientUreq::new(timeout);
        let authenticator = HttpAuthenticator::new(
            http_client,
            Url::parse(&identity_server.url(identity_server_path)).unwrap(),
        );
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

        let timeout = Duration::from_millis(100);
        let http_client = HttpClientUreq::new(timeout);
        let authenticator = HttpAuthenticator::new(
            http_client,
            Url::parse(&identity_server.url(identity_server_path)).unwrap(),
        );

        let error = authenticator.authenticate(request).unwrap_err();

        assert_matches!(error, AuthenticateError::HttpResponseError(401, _));
        mock.assert()
    }

    #[test]
    fn test_request_serialization_and_deserialization() {
        let request = Request {
            client_assertion: ClientAssertion::from("fake_assertion"),
            client_assertion_type: ClientAssertionType::JwtBearer,
            client_id: ClientID::from("fake_id"),
            grant_type: GrantType::ClientCredentials,
        };
        let serialized = r#"{"client_id":"fake_id","grant_type":"client_credentials","client_assertion_type":"urn:ietf:params:oauth:client-assertion-type:jwt-bearer","client_assertion":"fake_assertion"}"#;

        assert_eq!(serde_json::to_string(&request).unwrap(), serialized);
        assert_eq!(request, serde_json::from_str(serialized).unwrap());
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
