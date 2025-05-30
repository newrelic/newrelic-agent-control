use crate::agent_type::runtime_config::health_config::HttpHealth;
use crate::http::client::{HttpClient as InnerClient, HttpResponseError};
use crate::health::health_checker::{
    HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use crate::health::with_start_time::{HealthWithStartTime, StartTime};
use http::{HeaderName, HeaderValue, Request, Response};
use std::collections::HashMap;
use thiserror::Error;
use tracing::error;
use url::Url;

const DEFAULT_PROTOCOL: &str = "http://";

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum HttpClientError {
    #[error("internal HTTP client error: `{0}`")]
    HttpClientError(String),
}

/// The `HttpClient` trait defines the HTTP get interface to be implemented
/// by HTTP clients.
pub trait HttpClient {
    /// A synchronous function that defines the `get` method for HTTP client.
    fn get(
        &self,
        path: &str,
        headers: &HashMap<String, String>,
    ) -> Result<Response<Vec<u8>>, HttpClientError>;
}

impl HttpClient for InnerClient {
    fn get(
        &self,
        path: &str,
        headers: &HashMap<String, String>,
    ) -> Result<Response<Vec<u8>>, HttpClientError> {
        let mut request_builder = Request::builder().method("GET").uri(path);

        for (key, value) in headers {
            let header_name: HeaderName = key
                .parse::<HeaderName>()
                .map_err(|err| HttpResponseError::BuildingRequest(err.to_string()))?;
            let header_value: HeaderValue = value
                .parse::<HeaderValue>()
                .map_err(|err| HttpResponseError::BuildingRequest(err.to_string()))?;
            request_builder = request_builder.header(header_name, header_value);
        }

        let request = request_builder
            .body(Vec::new())
            .map_err(|err| HttpResponseError::BuildingRequest(err.to_string()))?;

        Ok(self.send(request)?)
    }
}
impl From<HttpResponseError> for HttpClientError {
    fn from(err: HttpResponseError) -> Self {
        HttpClientError::HttpClientError(err.to_string())
    }
}

/// The `HttpHealthChecker` is in charge of calling its client and parsing the health status
pub struct HttpHealthChecker<C = InnerClient>
where
    C: HttpClient,
{
    client: C,
    url: Url,
    headers: HashMap<String, String>,
    healthy_status_codes: Vec<u16>,
    start_time: StartTime,
}

impl HttpHealthChecker<InnerClient> {
    pub(crate) fn new(
        client: InnerClient,
        http_config: HttpHealth,
        start_time: StartTime,
    ) -> Result<Self, HealthCheckerError> {
        let host = format!(
            "{}{}",
            DEFAULT_PROTOCOL,
            String::from(http_config.host.get()),
        );

        let mut url =
            Url::parse(host.as_str()).map_err(|e| HealthCheckerError::Generic(e.to_string()))?;
        let _ = url.set_port(Some(http_config.port.get().into()));

        let path: String = http_config.path.get().into();
        url.set_path(path.as_str());

        let headers = http_config.headers;
        let healthy_status_codes = http_config.healthy_status_codes;

        Ok(Self {
            client,
            url,
            headers,
            healthy_status_codes,
            start_time,
        })
    }
}

impl<C: HttpClient> HealthChecker for HttpHealthChecker<C> {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        let response = self
            .client
            .get(self.url.as_str(), &self.headers)
            .map_err(|e| HealthCheckerError::Generic(e.to_string()))?;
        let status_code = response.status();

        let status = String::from_utf8_lossy(response.body()).into();

        if (self.healthy_status_codes.is_empty() && status_code.is_success())
            || self.healthy_status_codes.contains(&status_code.as_u16())
        {
            return Ok(HealthWithStartTime::from_healthy(
                Healthy::new(status),
                self.start_time,
            ));
        }

        let last_error = format!(
            "Health check failed with HTTP response status code {}",
            status_code
        );

        Ok(HealthWithStartTime::from_unhealthy(
            Unhealthy::new(status, last_error),
            self.start_time,
        ))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use http::Response;
    use mockall::mock;

    mock! {
        pub HttpClient {}
        impl HttpClient for HttpClient {
            fn get(&self, path: &str, headers: &HashMap<String, String>) -> Result<Response<Vec<u8>>, HttpClientError>;
        }
    }

    impl MockHttpClient {
        pub fn should_get(&mut self, response: Response<Vec<u8>>) {
            self.expect_get()
                .once()
                .return_once(move |_, _| Ok(response));
        }

        pub fn should_not_get(&mut self, error: HttpClientError) {
            self.expect_get().once().return_once(move |_, _| Err(error));
        }
    }

    #[test]
    fn http_client_error_unhealthy() {
        let mut client_mock = MockHttpClient::new();
        client_mock.should_not_get(HttpClientError::HttpClientError("Timeout".to_string()));

        let url = DEFAULT_PROTOCOL.to_owned() + "a-path";
        let checker = HttpHealthChecker {
            client: client_mock,
            url: Url::parse(url.as_str()).unwrap(),
            headers: Default::default(),
            healthy_status_codes: vec![],
            start_time: StartTime::now(),
        };

        let health_response = checker.check_health();

        assert!(health_response.is_err());
        assert_eq!(
            "internal HTTP client error: `Timeout`".to_string(),
            health_response.unwrap_err().to_string()
        );
    }

    #[test]
    fn empty_healthy_codes_healthy() {
        let mut client_mock = MockHttpClient::new();
        client_mock.should_get(
            http::Response::builder()
                .status(200)
                .body("ignored-body".as_bytes().to_vec())
                .unwrap(),
        );

        let url = DEFAULT_PROTOCOL.to_owned() + "a-path";
        let checker = HttpHealthChecker {
            client: client_mock,
            url: Url::parse(url.as_str()).unwrap(),
            headers: Default::default(),
            healthy_status_codes: vec![],
            start_time: StartTime::now(),
        };

        assert!(checker.check_health().is_ok());
    }

    #[test]
    fn empty_healthy_codes_unhealthy() {
        let mut client_mock = MockHttpClient::new();
        client_mock.should_get(
            http::Response::builder()
                .status(400)
                .body(http::StatusCode::BAD_REQUEST.as_str().as_bytes().to_vec())
                .unwrap(),
        );

        let url = DEFAULT_PROTOCOL.to_owned() + "a-path";
        let checker = HttpHealthChecker {
            client: client_mock,
            url: Url::parse(url.as_str()).unwrap(),
            headers: Default::default(),
            healthy_status_codes: vec![],
            start_time: StartTime::now(),
        };

        let health_response = checker.check_health();

        assert!(health_response.is_ok());
        assert_eq!(
            health_response.unwrap().status(),
            http::StatusCode::BAD_REQUEST.as_str()
        );
    }

    #[test]
    fn specific_healthy_codes() {
        let mut client_mock = MockHttpClient::new();
        client_mock.expect_get().times(1).returning(|_, _| {
            Ok(http::Response::builder()
                .status(201)
                .body(http::StatusCode::CREATED.as_str().as_bytes().to_vec())
                .unwrap())
        });

        let url = DEFAULT_PROTOCOL.to_owned() + "a-path";
        let mut checker = HttpHealthChecker {
            client: client_mock,
            url: Url::parse(url.as_str()).unwrap(),
            headers: Default::default(),
            healthy_status_codes: vec![200],
            start_time: StartTime::now(),
        };

        let health_response = checker.check_health();

        assert!(health_response.is_ok());
        assert_eq!(
            http::StatusCode::CREATED.as_str(),
            health_response.unwrap().status()
        );

        let mut client_mock = MockHttpClient::new();
        client_mock.expect_get().times(1).returning(|_, _| {
            Ok(http::Response::builder()
                .status(201)
                .body("response-body".as_bytes().to_vec())
                .unwrap())
        });

        checker = HttpHealthChecker {
            client: client_mock,
            url: Url::parse(url.as_str()).unwrap(),
            headers: Default::default(),
            healthy_status_codes: vec![201],
            start_time: StartTime::now(),
        };

        let health_response = checker.check_health();

        assert!(health_response.is_ok());

        let mut client_mock = MockHttpClient::new();
        client_mock.expect_get().times(1).returning(|_, _| {
            Ok(http::Response::builder()
                .status(501)
                .body("response-body".as_bytes().to_vec())
                .unwrap())
        });

        checker = HttpHealthChecker {
            client: client_mock,
            url: Url::parse(url.as_str()).unwrap(),
            headers: Default::default(),
            healthy_status_codes: vec![501],
            start_time: StartTime::now(),
        };

        let health_response = checker.check_health();

        assert!(health_response.is_ok());
    }
}
