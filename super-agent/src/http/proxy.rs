use http::Uri;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::env::{self, VarError};
use std::fmt::Display;
use std::path::{Path, PathBuf};

const HTTP_PROXY_ENV_NAME: &str = "HTTP_PROXY";
const HTTPS_PROXY_ENV_NAME: &str = "HTTPS_PROXY";

#[derive(thiserror::Error, Debug)]
pub enum ProxyError {
    #[error("invalid proxy url `{0}`: `{1}`")]
    InvalidUrl(String, String),
}

/// Type to represent an Url which can be used in proxy implementations.
/// It allows representing empty urls and perform basic uri validations.
#[derive(Debug, Default, PartialEq, Clone)]
struct ProxyUrl(Option<Uri>);

// We need custom Deserialize and Deserialize implementations since `http_serde` does not support Option, besides
// it allows us to customize the error.
impl<'de> Deserialize<'de> for ProxyUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: Option<String> = Option::deserialize(deserializer)?;
        let proxy_url = s
            .map(|s| Self::try_from(s.as_str()).map_err(serde::de::Error::custom))
            .transpose()?
            .unwrap_or_default();
        Ok(proxy_url)
    }
}

impl Serialize for ProxyUrl {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.0 {
            Some(url) => serializer.serialize_some(url.to_string().as_str()),
            None => serializer.serialize_none(),
        }
    }
}

impl TryFrom<&str> for ProxyUrl {
    type Error = ProxyError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s.is_empty() {
            return Ok(Self(None));
        }
        let uri = s
            .parse::<Uri>()
            .map_err(|err| ProxyError::InvalidUrl(s.to_string(), err.to_string()))?;
        Ok(Self(Some(uri)))
    }
}

impl Display for ProxyUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(url) => write!(f, "{url}"),
            None => write!(f, ""),
        }
    }
}

impl ProxyUrl {
    fn is_empty(&self) -> bool {
        self.0.is_none()
    }
}

/// Proxy for Super Agent HTTP Clients.
/// The priority of the proxy configuration is as follows:
///
/// NR__PROXY environment variable
/// proxy configuration option
/// HTTP_PROXY system environment variable
/// HTTPS_PROXY system environment variable
/// ```
/// # use newrelic_super_agent::http::proxy::ProxyConfig;
/// // The url will contain the value corresponding to the standard environment variables.
/// let proxy_config = ProxyConfig::default().try_with_url_from_env().unwrap();
/// ```
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Default)]
pub struct ProxyConfig {
    /// Proxy URL proxy:
    /// <protocol>://<user>:<password>@<host>:port
    /// (All parts except host are optional)
    ///
    /// Protocols supported:
    /// http: HTTP
    /// socks4: SOCKS4
    /// socks4a: SOCKS4A
    /// socks5 and socks: SOCKS5 (requires ureq socks feature)
    ///
    /// Examples from ureq:
    /// http://127.0.0.1:8080
    /// socks5://john:smith@socks.google.com
    /// john:smith@socks.google.com:8000
    /// localhost
    #[serde(default)]
    url: ProxyUrl,
    /// System path with the CA certificates in PEM format. All `.pem` files in the directory are read.
    #[serde(default)]
    ca_bundle_dir: PathBuf,
    /// System path with the CA certificate in PEM format.
    #[serde(default)]
    ca_bundle_file: PathBuf,
    // TODO : This is c&p from the Infra Agent. It might not be needed here?
    // If set to true, when the proxy is configured to use an HTTPS connection, it will only work:
    // * If the HTTPS proxy has certificates from a valid Certificate Authority.
    // * If the ca_bundle_file or ca_bundle_dir configuration properties contain the HTTPS proxy certificates.
    #[serde(default)]
    proxy_validate_certificates: bool,
    /// When set to true, the HTTPS_PROXY and HTTP_PROXY environment variables are ignored, defaults to false.
    #[serde(default)]
    ignore_system_proxy: bool,
}

impl ProxyConfig {
    pub fn ca_bundle_dir(&self) -> &Path {
        self.ca_bundle_dir.as_path()
    }

    pub fn ca_bundle_file(&self) -> &Path {
        self.ca_bundle_file.as_path()
    }

    /// Returns a string representation of the proxy url.
    pub fn url_as_string(&self) -> String {
        self.url.to_string()
    }

    /// Returns a new instance whose url is taken from the standard environment variables if needed.
    pub fn try_with_url_from_env(self) -> Result<Self, ProxyError> {
        Ok(Self {
            url: self.env_aware_url(env::var)?,
            ..self
        })
    }

    /// Returns the configured url, fetching the environment variable through the provided `env_var` function if
    /// required
    fn env_aware_url<F>(&self, env_var: F) -> Result<ProxyUrl, ProxyError>
    where
        F: Fn(&'static str) -> Result<String, VarError>,
    {
        if !self.url.is_empty() {
            return Ok(self.url.clone());
        }
        if self.ignore_system_proxy {
            return Ok(Default::default());
        }
        let url = env_var(HTTPS_PROXY_ENV_NAME)
            .or_else(|_| env_var(HTTP_PROXY_ENV_NAME))
            .unwrap_or_default()
            .as_str()
            .try_into()?;
        Ok(url)
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::ProxyError;
    use assert_matches::assert_matches;

    use super::{ProxyConfig, ProxyUrl};
    use std::{collections::HashMap, env::VarError, path::PathBuf};

    impl ProxyConfig {
        /// Convenient builder function for testing
        pub(crate) fn from_url(url: String) -> ProxyConfig {
            ProxyConfig {
                url: url.as_str().try_into().unwrap(),
                ..Default::default()
            }
        }
    }

    #[test]
    fn test_deserialize_proxy_config() {
        struct TestCase {
            _name: &'static str,
            content: &'static str,
            expected: ProxyConfig,
        }

        impl TestCase {
            fn run(self) {
                let actual = serde_yaml::from_str::<ProxyConfig>(self.content).unwrap();
                assert_eq!(self.expected, actual, "Test Name: {}", self._name);
            }
        }

        let test_cases = vec![
            TestCase {
                _name: "nothing",
                content: r#""#,
                expected: ProxyConfig::default(),
            },
            TestCase {
                _name: "just url",
                content: r#"url: "http://localhost:8888""#,
                expected: ProxyConfig {
                    url: "http://localhost:8888".try_into().unwrap(),
                    ..Default::default()
                },
            },
            TestCase {
                _name: "url with ca_bundle_dir",
                content: r#"
                    url: "http://localhost:8888"
                    ca_bundle_dir: "/path/to/ca_bundle"
                "#,
                expected: ProxyConfig {
                    url: "http://localhost:8888".try_into().unwrap(),
                    ca_bundle_dir: PathBuf::from("/path/to/ca_bundle"),
                    ..Default::default()
                },
            },
            TestCase {
                _name: "url with ca_bundle_file",
                content: r#"
                    url: "http://localhost:8888"
                    ca_bundle_file: "/path/to/ca_bundle.pem"
                "#,
                expected: ProxyConfig {
                    url: "http://localhost:8888".try_into().unwrap(),
                    ca_bundle_dir: PathBuf::default(),
                    ca_bundle_file: PathBuf::from("/path/to/ca_bundle.pem"),
                    ..Default::default()
                },
            },
            TestCase {
                _name: "url with proxy_validate_certificates",
                content: r#"
                    url: "http://localhost:8888"
                    proxy_validate_certificates: true
                "#,
                expected: ProxyConfig {
                    url: "http://localhost:8888".try_into().unwrap(),
                    proxy_validate_certificates: true,
                    ..Default::default()
                },
            },
            TestCase {
                _name: "url with ignore_system_proxy",
                content: r#"
                    url: "http://localhost:8888"
                    ignore_system_proxy: true
                "#,
                expected: ProxyConfig {
                    url: "http://localhost:8888".try_into().unwrap(),
                    ignore_system_proxy: true,
                    ..Default::default()
                },
            },
            TestCase {
                _name: "full configuration",
                content: r#"
                    url: "http://localhost:8888"
                    ca_bundle_dir: "/path/to/ca_bundle"
                    ca_bundle_file: "/path/to/ca_bundle.pem"
                    proxy_validate_certificates: true
                    ignore_system_proxy: true
                "#,
                expected: ProxyConfig {
                    url: "http://localhost:8888".try_into().unwrap(),
                    ca_bundle_dir: PathBuf::from("/path/to/ca_bundle"),
                    ca_bundle_file: PathBuf::from("/path/to/ca_bundle.pem"),
                    proxy_validate_certificates: true,
                    ignore_system_proxy: true,
                },
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn test_system_proxy_values() {
        struct TestCase {
            name: &'static str,
            env_values: HashMap<&'static str, &'static str>,
            config: ProxyConfig,
            expected: ProxyUrl,
        }

        impl TestCase {
            fn run(&self) {
                let url = self.config.env_aware_url(|k| {
                    self.env_values
                        .get(k)
                        .map(|v| v.to_string())
                        .ok_or(VarError::NotPresent)
                });
                assert_eq!(url.unwrap(), self.expected, "Test name {}", self.name)
            }
        }
        let test_cases = [
            TestCase {
                name: "No system proxy configured and no proxy in config",
                env_values: HashMap::from([("SOME_OTHER", "env-variable")]),
                config: ProxyConfig::default(),
                expected: ProxyUrl::default(),
            },
            TestCase {
                name: "No system proxy configured and proxy url",
                env_values: HashMap::from([("SOME_OTHER", "env-variable")]),
                config: ProxyConfig::from_url("http://localhost:8888".to_string()),
                expected: "http://localhost:8888".try_into().unwrap(),
            },
            TestCase {
                name: "Config url proxy has priority over system proxy",
                env_values: HashMap::from([("HTTPS_PROXY", "http://other.proxy:9999")]),
                config: ProxyConfig::from_url("http://localhost:8888".to_string()),
                expected: "http://localhost:8888".try_into().unwrap(),
            },
            TestCase {
                name: "HTTPS_PROXY env variable value is used",
                env_values: HashMap::from([("HTTPS_PROXY", "http://other.proxy:9999")]),
                config: ProxyConfig::default(),
                expected: "http://other.proxy:9999".try_into().unwrap(),
            },
            TestCase {
                name: "HTTP_PROXY env variable value is used",
                env_values: HashMap::from([("HTTP_PROXY", "http://other.proxy:9999")]),
                config: ProxyConfig::default(),
                expected: "http://other.proxy:9999".try_into().unwrap(),
            },
            TestCase {
                name: "HTTPS_PROXY has more priority",
                env_values: HashMap::from([
                    ("HTTPS_PROXY", "http://one.proxy:9999"),
                    ("HTTP_PROXY", "http://other.proxy:9999"),
                ]),
                config: ProxyConfig::default(),
                expected: "http://one.proxy:9999".try_into().unwrap(),
            },
            TestCase {
                name: "System proxy is ignored when the corresponding configuration is set",
                env_values: HashMap::from([
                    ("HTTPS_PROXY", "http://one.proxy:9999"),
                    ("HTTP_PROXY", "http://other.proxy:9999"),
                ]),
                config: ProxyConfig {
                    ignore_system_proxy: true,
                    ..Default::default()
                },
                expected: ProxyUrl::default(),
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }

    #[test]
    fn invalid_system_proxy() {
        let config = ProxyConfig::default();
        let result = config.env_aware_url(|_| Ok("http://".to_string()));
        assert_matches!(result.unwrap_err(), ProxyError::InvalidUrl(s, _) => {
            assert_eq!(s, "http://".to_string())
        });
    }
}
