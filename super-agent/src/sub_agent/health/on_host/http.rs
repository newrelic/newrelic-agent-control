use crate::agent_type::health_config::{
    HealthCheckTimeout, HttpHealth, OnHostHealthCheck, OnHostHealthConfig,
};
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use std::collections::HashMap;
use thiserror::Error;
use tracing::error;
use url::Url;

const DEFAULT_PROTOCOL: &str = "http://";

/// An enumeration of potential errors related to the HTTP client.
#[derive(Error, Debug)]
pub enum HttpClientError {
    /// Represents Ureq crate error.
    #[error("internal HTTP client error: `{0}`")]
    HttpClientError(String),
}

pub enum HealthCheckerType {
    Http(HttpHealthChecker),
}

impl TryFrom<OnHostHealthConfig> for HealthCheckerType {
    type Error = HealthCheckerError;

    fn try_from(health_config: OnHostHealthConfig) -> Result<Self, Self::Error> {
        let timeout = health_config.timeout;

        match health_config.check {
            OnHostHealthCheck::HttpHealth(http_config) => Ok(HealthCheckerType::Http(
                HttpHealthChecker::new(timeout, http_config)?,
            )),
        }
    }
}

impl HealthChecker for HealthCheckerType {
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        match self {
            HealthCheckerType::Http(http_checker) => http_checker.check_health(),
        }
    }
}

/// The `HttpClient` trait defines the HTTP get interface to be implemented
/// by HTTP clients.
pub trait HttpClient {
    /// A synchronous function that defines the `get` method for HTTP client.
    fn get(
        &self,
        path: &str,
        headers: &HashMap<String, String>,
    ) -> Result<http::Response<Vec<u8>>, HttpClientError>;
}

/// An implementation of the `HttpClient` trait using the ureq library.
impl HttpClient for ureq::Agent {
    fn get(
        &self,
        path: &str,
        headers: &HashMap<String, String>,
    ) -> Result<http::Response<Vec<u8>>, HttpClientError> {
        let mut req = self.get(path);

        for (header_name, header_value) in headers {
            req = req.set(header_name.as_str(), header_value.as_str());
        }

        match req.call() {
            Ok(response) | Err(ureq::Error::Status(_, response)) => Ok(response.into()),

            Err(ureq::Error::Transport(e)) => Err(HttpClientError::HttpClientError(e.to_string())),
        }
    }
}

/// The `HttpHealthChecker` is in charge of calling its client and parsing the health status
/// #[derive(Debug, Default)]
pub struct HttpHealthChecker<C = ureq::Agent>
where
    C: HttpClient,
{
    client: C,
    url: Url,
    headers: HashMap<String, String>,
    healthy_status_codes: Vec<u16>,
}

impl HttpHealthChecker<ureq::Agent> {
    pub(crate) fn new(
        timeout: HealthCheckTimeout,
        http_config: HttpHealth,
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
            client: ureq::AgentBuilder::new()
                .timeout_connect(timeout.into())
                .timeout(timeout.into())
                .build(),
            url,
            headers,
            healthy_status_codes,
        })
    }
}

impl<C: HttpClient> HealthChecker for HttpHealthChecker<C> {
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        let response = self
            .client
            .get(self.url.as_str(), &self.headers)
            .map_err(|e| HealthCheckerError::Generic(e.to_string()))?;
        let status_code = response.status();

        let status = String::from_utf8_lossy(response.body()).into();

        if (self.healthy_status_codes.is_empty() && status_code.is_success())
            || self.healthy_status_codes.contains(&status_code.as_u16())
        {
            return Ok(Healthy {
                status,
                ..Default::default()
            }
            .into());
        }

        let last_error = format!(
            "Health check failed with HTTP response status code {}",
            status_code
        );

        Ok(Unhealthy {
            status,
            last_error,
            ..Default::default()
        }
        .into())
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use http::Response;
    use mockall::mock;

    mock! {
        pub HttpClientMock {}
        impl HttpClient for HttpClientMock {
            fn get(&self, path: &str, headers: &HashMap<String, String>) -> Result<http::Response<Vec<u8>>, HttpClientError>;
        }
    }

    impl MockHttpClientMock {
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
        let mut client_mock = MockHttpClientMock::new();
        client_mock.should_not_get(HttpClientError::HttpClientError("Timeout".to_string()));

        let url = DEFAULT_PROTOCOL.to_owned() + "a-path";
        let checker = HttpHealthChecker {
            client: client_mock,
            url: Url::parse(url.as_str()).unwrap(),
            headers: Default::default(),
            healthy_status_codes: vec![],
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
        let mut client_mock = MockHttpClientMock::new();
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
        };

        assert!(checker.check_health().is_ok());
    }

    #[test]
    fn empty_healthy_codes_unhealthy() {
        let mut client_mock = MockHttpClientMock::new();
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
        };

        let health_response = checker.check_health();

        assert!(health_response.is_ok());
        assert_eq!(
            http::StatusCode::BAD_REQUEST.as_str(),
            health_response.unwrap().status()
        );
    }

    #[test]
    fn specific_healthy_codes() {
        let mut client_mock = MockHttpClientMock::new();
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
        };

        let health_response = checker.check_health();

        assert!(health_response.is_ok());
        assert_eq!(
            http::StatusCode::CREATED.as_str(),
            health_response.unwrap().status()
        );

        let mut client_mock = MockHttpClientMock::new();
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
        };

        let health_response = checker.check_health();

        assert!(health_response.is_ok());

        let mut client_mock = MockHttpClientMock::new();
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
        };

        let health_response = checker.check_health();

        assert!(health_response.is_ok());
    }
}
