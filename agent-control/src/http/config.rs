use crate::http::proxy::ProxyConfig;
use std::time::Duration;

#[derive(Default)]
pub struct HttpConfig {
    pub(crate) timeout: Duration,
    pub(crate) conn_timeout: Duration,
    pub(crate) proxy: ProxyConfig,
    pub(crate) tls_info: bool,
}

impl HttpConfig {
    pub fn new(timeout: Duration, conn_timeout: Duration, proxy: ProxyConfig) -> Self {
        Self {
            timeout,
            conn_timeout,
            proxy,
            tls_info: false,
        }
    }
    pub fn with_tls_info(self) -> Self {
        Self {
            tls_info: true,
            ..self
        }
    }
}
