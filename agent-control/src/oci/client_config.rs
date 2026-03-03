//! OCI client configuration implementing [oci_client::client::ClientConfigSource].

use super::OciClientError;
use crate::http::config::ProxyConfig;
use oci_client::client::{Certificate, ClientConfig, ClientConfigSource, ClientProtocol};

mod proxy;

/// Stores the configuration needed to build an [oci_client::Client].
///
/// Implements [ClientConfigSource] so an [oci_client::Client] can be constructed on demand
/// using [oci_client::Client::from_source] and dropped immediately after use.
pub(super) struct OciClientConfig {
    protocol: ClientProtocol,
    http_proxy: Option<String>,
    https_proxy: Option<String>,
    extra_root_certificates: Vec<Certificate>,
}

impl OciClientConfig {
    pub(super) fn try_new(
        client_config: ClientConfig,
        proxy_config: ProxyConfig,
    ) -> Result<Self, OciClientError> {
        let config = proxy::setup_proxy(client_config, proxy_config)?;
        Ok(Self {
            protocol: config.protocol,
            http_proxy: config.http_proxy,
            https_proxy: config.https_proxy,
            extra_root_certificates: config.extra_root_certificates,
        })
    }
}

impl ClientConfigSource for OciClientConfig {
    fn client_config(&self) -> ClientConfig {
        ClientConfig {
            protocol: self.protocol.clone(),
            http_proxy: self.http_proxy.clone(),
            https_proxy: self.https_proxy.clone(),
            extra_root_certificates: self.extra_root_certificates.clone(),
            ..Default::default()
        }
    }
}
