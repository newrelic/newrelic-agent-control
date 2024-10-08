use crate::http::proxy::ProxyConfig;
use std::time::Duration;

pub struct HttpConfig {
    timeout: Duration,
    conn_timeout: Duration,
    proxy_config: ProxyConfig,
}

impl HttpConfig {
    pub fn new(timeout: Duration, conn_timeout: Duration, proxy_config: ProxyConfig) -> Self {
        Self {
            timeout,
            conn_timeout,
            proxy_config,
        }
    }
}

impl HttpConfig {
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn conn_timeout(&self) -> Duration {
        self.conn_timeout
    }

    pub fn proxy_config(&self) -> ProxyConfig {
        self.proxy_config.clone()
    }
}

#[cfg(test)]
pub mod test {
    use crate::http::{config::HttpConfig, proxy::ProxyConfig};
    use std::time::Duration;

    #[allow(clippy::derivable_impls)] // implemented for tests only
    impl Default for HttpConfig {
        fn default() -> Self {
            HttpConfig {
                proxy_config: ProxyConfig::default(),
                timeout: Duration::default(),
                conn_timeout: Duration::default(),
            }
        }
    }
}
