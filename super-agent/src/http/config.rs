use crate::http::proxy::ProxyConfig;
use std::time::Duration;

#[derive(Default)]
pub struct HttpConfig {
    pub(crate) timeout: Duration,
    pub(crate) conn_timeout: Duration,
    pub(crate) proxy: ProxyConfig,
}

impl HttpConfig {
    pub fn new(timeout: Duration, conn_timeout: Duration, proxy: ProxyConfig) -> Self {
        Self {
            timeout,
            conn_timeout,
            proxy,
        }
    }
}
