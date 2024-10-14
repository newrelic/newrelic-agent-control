use http::{Request, Response};
use nr_auth::http_client::{HttpClient, HttpClientError};
use std::time::Duration;

/// Ureq implementation of HttpClient
pub struct HttpClientUreq {
    agent: ureq::Agent,
}

impl HttpClientUreq {
    pub fn new(timeout: Duration) -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(timeout)
                .timeout(timeout)
                .build(),
        }
    }
}

impl HttpClient for HttpClientUreq {
    fn send(&self, request: Request<Vec<u8>>) -> Result<Response<Vec<u8>>, HttpClientError> {
        // Build the ureq request from the agent to get the configs set in there.
        // The .into() method from conversion would create a new agent per request so we avoid that.
        let mut req = self.agent.request(
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
            Ok(response) | Err(ureq::Error::Status(_, response)) => build_response(response),

            Err(ureq::Error::Transport(e)) => Err(HttpClientError::TransportError(e.to_string())),
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
    response
        .into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| HttpClientError::InvalidResponse(e.to_string()))?;

    response_builder
        .body(buf)
        .map_err(|e| HttpClientError::InvalidResponse(e.to_string()))
}
