use crate::http::config::HttpConfig;
use crate::http::tls::build_tls_config;
use crate::http::ureq::UreqClientBuilderError::BuildingError;
use http::Response;
use std::sync::Arc;
use ureq::{Agent, Proxy};

#[derive(thiserror::Error, Debug)]
pub enum UreqClientBuilderError {
    #[error("`{0}`")]
    BuildingError(String),
}

#[derive(thiserror::Error, Debug)]
pub enum UreqResponseError {
    #[error("error parsing response: `{0}`")]
    ErrorParsingResponse(String),
    #[error("error building response: `{0}`")]
    ErrorBuildingResponse(String),
}

pub fn try_build_ureq(config: HttpConfig) -> Result<Agent, UreqClientBuilderError> {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(config.conn_timeout())
        .timeout(config.timeout());

    let proxy_conf = config.proxy_config();
    let proxy_url = proxy_conf.url();
    if !proxy_url.is_empty() {
        let proxy = Proxy::new(proxy_url)
            .map_err(|x| BuildingError(format!(" invalid proxy url: {}", x)))?;

        let tls_config = build_tls_config(proxy_conf.ca_bundle_file(), proxy_conf.ca_bundle_dir())
            .map_err(|e| BuildingError(format!("error building tls: {}", e)))?;

        builder = builder.proxy(proxy).tls_config(Arc::new(tls_config));
    }

    Ok(builder.build())
}

pub fn build_response(response: ureq::Response) -> Result<Response<Vec<u8>>, UreqResponseError> {
    let http_version = match response.http_version() {
        "HTTP/0.9" => http::Version::HTTP_09,
        "HTTP/1.0" => http::Version::HTTP_10,
        "HTTP/1.1" => http::Version::HTTP_11,
        "HTTP/2.0" => http::Version::HTTP_2,
        "HTTP/3.0" => http::Version::HTTP_3,
        _ => unreachable!(),
    };

    let response_builder = http::Response::builder()
        .status(response.status())
        .version(http_version);

    let mut buf: Vec<u8> = vec![];
    response
        .into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| UreqResponseError::ErrorParsingResponse(e.to_string()))?;

    response_builder
        .body(buf)
        .map_err(|e| UreqResponseError::ErrorBuildingResponse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::StatusCode;
    use httpmock::MockServer;

    use crate::http::{config::HttpConfig, proxy::ProxyConfig};

    use super::try_build_ureq;

    #[test]
    fn test_ureq_proxy() {
        // Target server simulating the real service
        let expected_response = "OK!";
        let target_server = MockServer::start();
        target_server.mock(|when, then| {
            when.any_request();
            then.status(200).body(expected_response);
        });
        // Proxy server will request the target server, allowing requests to that host only
        let proxy_server = MockServer::start();
        proxy_server.proxy(|rule| {
            rule.filter(|when| {
                when.host(target_server.host()).port(target_server.port());
            });
        });
        // Build a ureq client using the proxy configuration
        let config = HttpConfig::new(
            Duration::from_secs(3),
            Duration::from_secs(3),
            ProxyConfig::from_url(proxy_server.base_url()),
        );
        let agent = try_build_ureq(config)
            .unwrap_or_else(|e| panic!("Unexpected error building ureq client {e}"));
        let resp = agent
            .get(target_server.url("/path").as_str())
            .call()
            .unwrap_or_else(|e| panic!("Error performing request: {e}"));
        // Check responses from the target server
        assert_eq!(resp.status(), StatusCode::OK.as_u16());
        assert_eq!(resp.into_string().unwrap(), expected_response.to_string())
    }
}
