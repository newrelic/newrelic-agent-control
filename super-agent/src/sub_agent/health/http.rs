use super::health_checker::{HealthChecker, HealthCheckerError};
use crate::agent_type::health_config::{HttpHealth, HttpHost};
use std::collections::HashMap;
use std::time::Duration;
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

/// The `HttpClient` trait defines the HTTP get interface to be implemented
/// by HTTP clients.
pub trait HttpClient {
    /// A synchronous function that defines the `post` method for HTTP client.
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

        Ok(req
            .call()
            .map_err(|e| HttpClientError::HttpClientError(e.to_string()))?
            .into())
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
    interval: Duration,
    healthy_status_codes: Vec<u16>,
}

impl HttpHealthChecker<ureq::Agent> {
    pub(crate) fn new(
        interval: Duration,
        timeout: Duration,
        http_config: HttpHealth,
    ) -> Result<Self, HealthCheckerError> {
        let host = format!(
            "{}{}",
            DEFAULT_PROTOCOL,
            <HttpHost as Into<String>>::into(http_config.host.get())
        );

        let mut url = Url::parse(host.as_str())
            .map_err(|e| HealthCheckerError::new("".to_string(), e.to_string()))?;
        let _ = url.set_port(Some(http_config.port.get().into()));

        let path: String = http_config.path.get().into();
        url.set_path(path.as_str());

        let headers = http_config.headers;
        let healthy_status_codes = http_config.healthy_status_codes;

        Ok(Self {
            client: ureq::AgentBuilder::new()
                .timeout_connect(timeout)
                .timeout(timeout)
                .build(),
            url,
            headers,
            interval,
            healthy_status_codes,
        })
    }
}

impl<C: HttpClient> HealthChecker for HttpHealthChecker<C> {
    fn check_health(&self) -> Result<(), HealthCheckerError> {
        let response = self.client.get(self.url.as_str(), &self.headers);
        match response {
            Ok(response) => {
                let status = response.status();
                if (self.healthy_status_codes.is_empty() && status.is_success())
                    || self.healthy_status_codes.contains(&status.as_u16())
                {
                    return Ok(());
                }

                let last_err = String::from_utf8(response.body().to_vec())
                    .map_err(|e| HealthCheckerError::new("".to_string(), e.to_string()))?;

                Err(HealthCheckerError::new(status.to_string(), last_err))
            }
            Err(err) => Err(HealthCheckerError::new("".to_string(), err.to_string())),
        }
    }

    fn interval(&self) -> Duration {
        self.interval
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
            interval: Default::default(),
            healthy_status_codes: vec![],
        };

        let health_response = checker.check_health();

        assert!(health_response.is_err());
        assert_eq!(
            "internal HTTP client error: `Timeout`".to_string(),
            health_response.unwrap_err().last_error()
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
            interval: Default::default(),
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
                .body("error-body".as_bytes().to_vec())
                .unwrap(),
        );

        let url = DEFAULT_PROTOCOL.to_owned() + "a-path";
        let checker = HttpHealthChecker {
            client: client_mock,
            url: Url::parse(url.as_str()).unwrap(),
            headers: Default::default(),
            interval: Default::default(),
            healthy_status_codes: vec![],
        };

        let health_response = checker.check_health();

        assert!(health_response.is_err());
        assert_eq!(
            "error-body".to_string(),
            health_response.unwrap_err().last_error()
        );
    }

    #[test]
    fn specific_healthy_codes() {
        let mut client_mock = MockHttpClientMock::new();
        client_mock.expect_get().times(1).returning(|_, _| {
            Ok(http::Response::builder()
                .status(201)
                .body("response-body".as_bytes().to_vec())
                .unwrap())
        });

        let url = DEFAULT_PROTOCOL.to_owned() + "a-path";
        let mut checker = HttpHealthChecker {
            client: client_mock,
            url: Url::parse(url.as_str()).unwrap(),
            headers: Default::default(),
            interval: Default::default(),
            healthy_status_codes: vec![200],
        };

        let health_response = checker.check_health();

        assert!(health_response.is_err());
        assert_eq!(
            "response-body".to_string(),
            health_response.unwrap_err().last_error()
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
            interval: Default::default(),
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
            interval: Default::default(),
            healthy_status_codes: vec![501],
        };

        let health_response = checker.check_health();

        assert!(health_response.is_ok());
    }
}
