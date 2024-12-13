use crate::http::ureq::build_response;
use http::Response;
use nr_auth::http_client::{HttpClient, HttpClientError};
use ureq::Agent;

pub struct AuthHttpClient {
    client: Agent,
}

impl AuthHttpClient {
    pub fn new(client: Agent) -> Self {
        Self { client }
    }
}

impl HttpClient for AuthHttpClient {
    fn send(&self, request: http::Request<Vec<u8>>) -> Result<Response<Vec<u8>>, HttpClientError> {
        // Build the ureq request from the agent to get the configs set in there.
        // The .into() method from conversion would create a new agent per request so we avoid that.
        let mut req = self.client.request(
            request.method().as_str(),
            request.uri().to_string().as_str(),
        );
        for (header_name, header_value) in request.headers() {
            let header_val = header_value.to_str().map_err(|e| {
                HttpClientError::EncoderError(format!("setting request header: {}", e))
            })?;
            req = req.set(header_name.as_str(), header_val);
        }
        match req.send_bytes(request.body()) {
            Ok(response) | Err(ureq::Error::Status(_, response)) => build_response(response)
                .map_err(|err| HttpClientError::InvalidResponse(err.to_string())),
            Err(ureq::Error::Transport(e)) => Err(HttpClientError::TransportError(e.to_string())),
        }
    }
}
