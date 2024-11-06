use serde::{Deserialize, Serialize};
use std::env::{self, VarError};
use std::path::{Path, PathBuf};

const HTTP_PROXY_ENV_NAME: &str = "HTTP_PROXY";
const HTTPS_PROXY_ENV_NAME: &str = "HTTPS_PROXY";

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
/// let proxy_config = ProxyConfig::default().with_url_from_env();
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
    /// socks5 and socks: SOCKS5 (requires socks feature)
    ///
    /// Examples from ureq:
    /// http:// 127.0.0.1:8080
    /// socks5:// john:smith@socks. google. com
    /// john:smith@socks. google. com:8000
    /// localhost
    #[serde(default)]
    url: String,
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

    /// Returns the configured url according to configuration, this includes the value from environment variables if
    /// it applies.
    pub fn url(&self) -> String {
        self.url.clone()
    }

    /// Returns a new instance whose url is taken from the standard environment variables if needed.
    pub fn with_url_from_env(self) -> Self {
        Self {
            url: self.env_aware_url(env::var),
            ..self
        }
    }

    /// Returns the configured url, fetching the environment variable through the provided `env_var` function if
    /// required
    fn env_aware_url<F>(&self, env_var: F) -> String
    where
        F: Fn(&'static str) -> Result<String, VarError>,
    {
        if !self.url.is_empty() {
            return self.url.clone();
        }
        if self.ignore_system_proxy {
            return Default::default();
        }
        env_var(HTTPS_PROXY_ENV_NAME)
            .or_else(|_| env_var(HTTP_PROXY_ENV_NAME))
            .unwrap_or_default()
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::ProxyConfig;
    use std::{collections::HashMap, env::VarError, path::PathBuf};

    impl ProxyConfig {
        /// Convenient builder function for testing
        pub(crate) fn from_url(url: String) -> ProxyConfig {
            ProxyConfig {
                url,
                ..Default::default()
            }
        }
    }

    #[test]
    fn test_deserialize_proxy() {
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
                expected: ProxyConfig {
                    url: String::default(),
                    ca_bundle_dir: PathBuf::default(),
                    ca_bundle_file: PathBuf::default(),
                    proxy_validate_certificates: false,
                    ignore_system_proxy: false,
                },
            },
            TestCase {
                _name: "just url",
                content: r#"url: "http://localhost:8888""#,
                expected: ProxyConfig {
                    url: "http://localhost:8888".to_string(),
                    ca_bundle_dir: PathBuf::default(),
                    ca_bundle_file: PathBuf::default(),
                    proxy_validate_certificates: false,
                    ignore_system_proxy: false,
                },
            },
            TestCase {
                _name: "url with ca_bundle_dir",
                content: r#"
                    url: "http://localhost:8888"
                    ca_bundle_dir: "/path/to/ca_bundle"
                "#,
                expected: ProxyConfig {
                    url: "http://localhost:8888".to_string(),
                    ca_bundle_dir: PathBuf::from("/path/to/ca_bundle"),
                    ca_bundle_file: PathBuf::default(),
                    proxy_validate_certificates: false,
                    ignore_system_proxy: false,
                },
            },
            TestCase {
                _name: "url with ca_bundle_file",
                content: r#"
                    url: "http://localhost:8888"
                    ca_bundle_file: "/path/to/ca_bundle.pem"
                "#,
                expected: ProxyConfig {
                    url: "http://localhost:8888".to_string(),
                    ca_bundle_dir: PathBuf::default(),
                    ca_bundle_file: PathBuf::from("/path/to/ca_bundle.pem"),
                    proxy_validate_certificates: false,
                    ignore_system_proxy: false,
                },
            },
            TestCase {
                _name: "url with proxy_validate_certificates",
                content: r#"
                    url: "http://localhost:8888"
                    proxy_validate_certificates: true
                "#,
                expected: ProxyConfig {
                    url: "http://localhost:8888".to_string(),
                    ca_bundle_dir: PathBuf::default(),
                    ca_bundle_file: PathBuf::default(),
                    proxy_validate_certificates: true,
                    ignore_system_proxy: false,
                },
            },
            TestCase {
                _name: "url with ignore_system_proxy",
                content: r#"
                    url: "http://localhost:8888"
                    ignore_system_proxy: true
                "#,
                expected: ProxyConfig {
                    url: "http://localhost:8888".to_string(),
                    ca_bundle_dir: PathBuf::default(),
                    ca_bundle_file: PathBuf::default(),
                    proxy_validate_certificates: false,
                    ignore_system_proxy: true,
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
                    url: "http://localhost:8888".to_string(),
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
            expected_url: String,
        }

        impl TestCase {
            fn run(&self) {
                let url = self.config.env_aware_url(|k| {
                    self.env_values
                        .get(k)
                        .map(|v| v.to_string())
                        .ok_or(VarError::NotPresent)
                });
                assert_eq!(url, self.expected_url, "Test name {}", self.name)
            }
        }
        let test_cases = [
            TestCase {
                name: "No system proxy configured and no proxy in config",
                env_values: HashMap::from([("SOME_OTHER", "env-variable")]),
                config: ProxyConfig::default(),
                expected_url: String::default(),
            },
            TestCase {
                name: "No system proxy configured and proxy url",
                env_values: HashMap::from([("SOME_OTHER", "env-variable")]),
                config: ProxyConfig::from_url("http://localhost:8888".to_string()),
                expected_url: "http://localhost:8888".to_string(),
            },
            TestCase {
                name: "Config url proxy has priority over system proxy",
                env_values: HashMap::from([("HTTPS_PROXY", "http://other.proxy:9999")]),
                config: ProxyConfig::from_url("http://localhost:8888".to_string()),
                expected_url: "http://localhost:8888".to_string(),
            },
            TestCase {
                name: "HTTPS_PROXY env variable value is used",
                env_values: HashMap::from([("HTTPS_PROXY", "http://other.proxy:9999")]),
                config: ProxyConfig::default(),
                expected_url: "http://other.proxy:9999".to_string(),
            },
            TestCase {
                name: "HTTP_PROXY env variable value is used",
                env_values: HashMap::from([("HTTP_PROXY", "http://other.proxy:9999")]),
                config: ProxyConfig::default(),
                expected_url: "http://other.proxy:9999".to_string(),
            },
            TestCase {
                name: "HTTPS_PROXY has more priority",
                env_values: HashMap::from([
                    ("HTTPS_PROXY", "http://one.proxy:9999"),
                    ("HTTP_PROXY", "http://other.proxy:9999"),
                ]),
                config: ProxyConfig::default(),
                expected_url: "http://one.proxy:9999".to_string(),
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
                expected_url: String::default(),
            },
        ];

        for test_case in test_cases {
            test_case.run();
        }
    }
}
