use http::{HeaderMap, Response};
use serde::Serialize;
use std::time::Duration;
use thiserror::Error;
use tracing::{error, info};
use ureq::Error;

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum HttpClientError {
    /// Represents Ureq crate error.
    #[error("internal HTTP client error: `{0}`")]
    UreqError(String),
}

/// The `HttpClient` trait defines the HTTP get interface to be implemented
/// by HTTP clients.
pub trait HttpClient {
    /// Returns a `http::Response<Vec<u8>>` structure as the HTPP response or
    /// HttpClientError if an error was found.
    fn post(&self, data: impl serde::Serialize)
        -> Result<http::Response<Vec<u8>>, HttpClientError>;
}

/// An implementation of the `HttpClient` trait using the ureq library.
pub struct HttpClientUreq {
    client: ureq::Agent,
    url: String,
    header_map: HeaderMap,
}

impl HttpClientUreq {
    /// Returns a new instance of HttpClientUreq
    pub fn new(url: String, timeout: Duration, header_map: HeaderMap) -> Self {
        Self {
            client: ureq::AgentBuilder::new()
                .timeout_connect(timeout)
                .timeout(timeout)
                .build(),
            url,
            header_map,
        }
    }
}

#[derive(Serialize)]
struct FakeResponse {
    access_token: String,
    expires_in: u32,
    token_type: String,
}

impl HttpClient for HttpClientUreq {
    fn post(
        &self,
        data: impl serde::Serialize,
    ) -> Result<http::Response<Vec<u8>>, HttpClientError> {
        let mut req = self.client.post(&self.url);

        for (header_name, header_value) in self.header_map.iter() {
            if let Ok(value) = header_value.to_str() {
                req = req.set(header_name.as_str(), value);
            } else {
                error!("invalid header value for {}", header_name)
            }
        }

        //<Real call>
        Ok(req
            .send_json(data)
            .map_err(|e| {
                let error_msg = match e {
                    Error::Status(code, resp) => {
                        format!("Status code: {}, response {:?}", code, resp.into_string())
                    }
                    Error::Transport(e) => format!("Transport error: {}", e),
                };
                HttpClientError::UreqError(error_msg)
            })?
            .into())

        // //<Fake call>
        // info!("request to auth with request: {:?}", req);
        // let fake_response = FakeResponse {
        //     token_type: String::from("Bearer"),
        //     expires_in: 3600,
        //     access_token: String::from("access-token-from-auth-system"),
        // };
        // let response = Response::new(serde_json::to_vec(&fake_response).unwrap());
        // Ok(response)
    }
}
