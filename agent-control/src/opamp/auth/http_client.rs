use crate::http::reqwest::try_build_response;
use http::Response;
use nr_auth::http_client::{HttpClient, HttpClientError};

pub struct ReqwestAuthHttpClient {
    client: reqwest::blocking::Client,
}

impl ReqwestAuthHttpClient {
    pub fn new(client: reqwest::blocking::Client) -> Self {
        Self { client }
    }
}

impl HttpClient for ReqwestAuthHttpClient {
    fn send(&self, request: http::Request<Vec<u8>>) -> Result<Response<Vec<u8>>, HttpClientError> {
        let req_body: Vec<u8> = request.body().to_vec();
        let req = self
            .client
            .request(request.method().into(), request.uri().to_string().as_str())
            .headers(request.headers().clone())
            .body(req_body);

        let res = req
            .send()
            .map_err(|err| HttpClientError::TransportError(err.to_string()))?;

        Ok(try_build_response(res)?)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::Method;
    use httpmock::MockServer;
    use nr_auth::http_client::HttpClient;

    use crate::{
        http::{config::HttpConfig, reqwest::try_build_reqwest_client},
        opamp::auth::http_client::ReqwestAuthHttpClient,
    };

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
                let mock_server = MockServer::start();
                let path = "/";
                let uri = mock_server.url(path);
                let method = Method::PUT;
                let (header_name, header_value) = ("key".to_string(), "value".to_string());
                let req_body_content = "body_content";
                let req_body = req_body_content.to_string().clone().as_bytes().to_vec();
                let resp_body = self.name.to_string();

                // Build the request we are sending through the auth client
                let request = http::Request::builder()
                    .uri(uri)
                    .method(method.clone())
                    .header(header_name.clone(), header_value.clone())
                    .body(req_body.clone())
                    .unwrap();

                // Set up the mock server expectations
                let req_mock = mock_server.mock(|when, then| {
                    when.path(path)
                        .method(method.as_str())
                        .header(header_name, header_value)
                        .body(req_body_content);

                    then.status(self.status_code).body(resp_body);
                });

                // Build the client
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
                let client = ReqwestAuthHttpClient::new(reqwest_client);

                // Perform request
                let res = client.send(request).unwrap_or_else(|err| {
                    panic!(
                        "unexpected error performing the request: {} - {}",
                        err, self.name
                    )
                });

                // Assert response content and mock-server calls
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
                req_mock.assert_calls(1);
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
