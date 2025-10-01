//! # Helpers to build a reqwest blocking client and handle responses and handle responses
//!
use crate::http::config::HttpConfig;
use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response};
use http::{Response as HttpResponse, StatusCode};
use nr_auth::http_client::HttpClient as OauthHttpClient;
use nr_auth::http_client::HttpClientError as OauthHttpClientError;
use opamp_client::http::HttpClientError as OpampHttpClientError;
use opentelemetry_http::HttpError;
use reqwest::tls::TlsInfo;
use reqwest::{
    Certificate, Error as ReqwestError, Proxy,
    blocking::{Client, Response as BlockingResponse},
};
use resource_detection::cloud::http_client::HttpClient as CloudClient;
use resource_detection::cloud::http_client::HttpClientError as CloudClientError;
use std::{
    fmt::Display,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use tracing::warn;

const CERT_EXTENSION: &str = "pem";
#[derive(Debug, Clone)]
pub struct HttpClient {
    client: Client,
}
impl HttpClient {
    /// Builds a reqwest blocking client according to the provided configuration.
    pub fn new(http_config: HttpConfig) -> Result<Self, HttpBuildError> {
        let mut builder = Client::builder()
            .use_rustls_tls() // Use rust-tls backend
            .tls_built_in_native_certs(true) // Load system (native) certificates
            .timeout(http_config.timeout)
            .connect_timeout(http_config.conn_timeout);

        if http_config.tls_info {
            builder = builder.tls_info(true);
        }

        let proxy_config = http_config.proxy;
        let proxy_url = proxy_config.url_as_string();
        if !proxy_url.is_empty() {
            let proxy = Proxy::all(proxy_url).map_err(|err| {
                HttpBuildError::ClientBuilder(format!("invalid proxy url: {err}"))
            })?;
            builder = builder.proxy(proxy);
            for cert in
                certs_from_paths(proxy_config.ca_bundle_file(), proxy_config.ca_bundle_dir())?
            {
                builder = builder.add_root_certificate(cert)
            }
        }

        let client = builder
            .build()
            .map_err(|err| HttpBuildError::ClientBuilder(err.to_string()))?;
        Ok(Self { client })
    }

    pub fn send(
        &self,
        request: Request<Vec<u8>>,
    ) -> Result<HttpResponse<Vec<u8>>, HttpResponseError> {
        let req_builder = self
            .client
            .request(request.method().into(), request.uri().to_string().as_str())
            .headers(request.headers().clone())
            .body(request.body().to_vec());

        let res = req_builder.send().map_err(from_reqwest_error)?;

        if res.status().is_success() {
            try_build_response(res)
        } else {
            let status_code = res.status();
            let body = res
                .bytes()
                .map_err(|err| HttpResponseError::ReadingResponse(err.to_string()))?
                .to_vec();
            Err(HttpResponseError::UnsuccessfulResponse { status_code, body })
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HttpResponseError {
    #[error("could read response body: {0}")]
    ReadingResponse(String),
    #[error("could build response: {0}")]
    BuildingResponse(String),
    #[error("could build request: {0}")]
    BuildingRequest(String),
    /// Represents a response that was received, but had a non-successful status code.
    #[error(
        "unsuccessful response: {status_code} - body: {}",
        String::from_utf8_lossy(body)
    )]
    UnsuccessfulResponse {
        status_code: StatusCode,
        body: Vec<u8>,
    },
    #[error(
        "connection error: could not connect to the host. this is often caused by a firewall, proxy, or network routing issue. original error: {0}"
    )]
    ConnectError(#[source] ReqwestError),
    #[error("timeout error: the request timed out. original error: {0}")]
    TimeoutError(#[source] ReqwestError),
    #[error(
        "dns resolution error: could not resolve the host. please check your dns configuration. original error: {0}"
    )]
    DnsError(#[source] ReqwestError),
    #[error("generic transport error: {0}")]
    GenericTransportError(#[source] ReqwestError),
}
fn from_reqwest_error(e: ReqwestError) -> HttpResponseError {
    if e.is_connect() {
        HttpResponseError::ConnectError(e)
    } else if e.is_timeout() {
        HttpResponseError::TimeoutError(e)
    } else if e.is_builder() || e.is_request() {
        if e.to_string().to_lowercase().contains("dns") {
            HttpResponseError::DnsError(e)
        } else {
            HttpResponseError::BuildingRequest(e.to_string())
        }
    } else {
        HttpResponseError::GenericTransportError(e)
    }
}

impl From<HttpResponseError> for OpampHttpClientError {
    fn from(err: HttpResponseError) -> Self {
        match err {
            HttpResponseError::ConnectError(_)
            | HttpResponseError::TimeoutError(_)
            | HttpResponseError::DnsError(_)
            | HttpResponseError::GenericTransportError(_) => {
                OpampHttpClientError::TransportError(err.to_string())
            }
            HttpResponseError::UnsuccessfulResponse { status_code, body } => {
                let msg = format!(
                    "HTTP Error {}: {}",
                    status_code,
                    String::from_utf8_lossy(&body)
                );
                OpampHttpClientError::HTTPBodyError(msg)
            }
            HttpResponseError::BuildingRequest(msg)
            | HttpResponseError::BuildingResponse(msg)
            | HttpResponseError::ReadingResponse(msg) => OpampHttpClientError::HTTPBodyError(msg),
        }
    }
}

impl CloudClient for HttpClient {
    fn send(&self, request: Request<Vec<u8>>) -> Result<HttpResponse<Vec<u8>>, CloudClientError> {
        Ok(self.send(request)?)
    }
}

impl From<HttpResponseError> for CloudClientError {
    fn from(err: HttpResponseError) -> Self {
        match err {
            HttpResponseError::UnsuccessfulResponse { status_code, body } => {
                CloudClientError::ResponseError(
                    status_code.into(),
                    String::from_utf8_lossy(&body).to_string(),
                )
            }
            other_error => CloudClientError::TransportError(other_error.to_string()),
        }
    }
}

impl OauthHttpClient for HttpClient {
    fn send(&self, req: Request<Vec<u8>>) -> Result<Response<Vec<u8>>, OauthHttpClientError> {
        let response = self.send(req)?;

        Ok(response)
    }
}

impl From<HttpResponseError> for OauthHttpClientError {
    fn from(err: HttpResponseError) -> Self {
        match err {
            HttpResponseError::ConnectError(_)
            | HttpResponseError::TimeoutError(_)
            | HttpResponseError::DnsError(_)
            | HttpResponseError::GenericTransportError(_) => {
                OauthHttpClientError::TransportError(err.to_string())
            }
            HttpResponseError::UnsuccessfulResponse { status_code, body } => {
                let msg = format!(
                    "HTTP Error {}: {}",
                    status_code,
                    String::from_utf8_lossy(&body)
                );
                OauthHttpClientError::InvalidResponse(msg)
            }
            HttpResponseError::BuildingRequest(msg)
            | HttpResponseError::BuildingResponse(msg)
            | HttpResponseError::ReadingResponse(msg) => OauthHttpClientError::InvalidResponse(msg),
        }
    }
}

// Implements opentelemetry_http HttpClient so it can be injected to an opentelemetry_otlp exporter
#[async_trait]
impl opentelemetry_http::HttpClient for HttpClient {
    async fn send_bytes(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        let (parts, body) = request.into_parts();
        let req_vec = Request::from_parts(parts, Vec::from(body));

        let response_vec = self.send(req_vec)?;

        let (parts, body) = response_vec.into_parts();
        Ok(Response::from_parts(parts, Bytes::from(body)))
    }
}

/// Helper to build a [HttpResponse<Vec<u8>>] from a reqwest's blocking response.
/// It includes status, version and body. Headers are not included but they could be added if needed.
fn try_build_response(res: BlockingResponse) -> Result<HttpResponse<Vec<u8>>, HttpResponseError> {
    let status = res.status();
    let version = res.version();

    let tls_info = res.extensions().get::<TlsInfo>().cloned();

    let body: Vec<u8> = res
        .bytes()
        .map_err(|err| HttpResponseError::ReadingResponse(err.to_string()))?
        .into();

    let mut response_builder = http::Response::builder().status(status).version(version);

    if let Some(tls_info) = tls_info {
        response_builder = response_builder.extension(tls_info);
    }

    let response = response_builder
        .body(body)
        .map_err(|err| HttpResponseError::BuildingResponse(err.to_string()))?;

    Ok(response)
}

#[derive(thiserror::Error, Debug)]
pub enum HttpBuildError {
    #[error("could not build the http client: {0}")]
    ClientBuilder(String),
    #[error("could not load certificates from {path}: {err}")]
    CertificateError { path: String, err: String },
}

/// Tries to extract certificates from the provided `ca_bundle_file` and `ca_bundle_dir` paths.
fn certs_from_paths(
    ca_bundle_file: &Path,
    ca_bundle_dir: &Path,
) -> Result<Vec<Certificate>, HttpBuildError> {
    let mut certs = Vec::new();
    // Certs from bundle file
    certs.extend(certs_from_file(ca_bundle_file)?);
    // Certs from bundle dir
    for path in cert_paths_from_dir(ca_bundle_dir)? {
        certs.extend(certs_from_file(&path)?)
    }
    Ok(certs)
}

/// Returns all certs bundled in the file corresponding to the provided path.
fn certs_from_file(path: &Path) -> Result<Vec<Certificate>, HttpBuildError> {
    if path.as_os_str().is_empty() {
        return Ok(Vec::new());
    }
    let mut buf = Vec::new();
    File::open(path)
        .map_err(|err| certificate_error(path, err))?
        .read_to_end(&mut buf)
        .map_err(|err| certificate_error(path, err))?;
    let certs = Certificate::from_pem_bundle(&buf).map_err(|err| certificate_error(path, err))?;
    Ok(certs)
}

/// Returns all paths to be considered to load certificates under the provided directory path.
fn cert_paths_from_dir(dir_path: &Path) -> Result<Vec<PathBuf>, HttpBuildError> {
    if dir_path.as_os_str().is_empty() {
        return Ok(Vec::new());
    }
    let dir_entries =
        std::fs::read_dir(dir_path).map_err(|err| certificate_error(dir_path, err))?;
    // filter readable file with 'cert' extension
    let paths = dir_entries.filter_map(|entry_res| match entry_res {
        Err(err) => {
            warn!(%err, directory=dir_path.to_string_lossy().to_string(), "Unreadable path when loading certificates from directory");
            None
        }
        Ok(entry) => {
            let path = entry.path();
            path_has_cert_extension(&path).then_some(path)
        }
    });
    Ok(paths.collect())
}

/// Helper to build a [HttpBuildError::CertificateError] more concisely.
fn certificate_error<E: Display>(path: &Path, err: E) -> HttpBuildError {
    HttpBuildError::CertificateError {
        path: path.to_string_lossy().into(),
        err: err.to_string(),
    }
}

/// Checks if the provided path has certificate extension
fn path_has_cert_extension(path: &Path) -> bool {
    match path.extension() {
        Some(extension) => extension == CERT_EXTENSION,
        None => false,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::http::config::ProxyConfig;
    use assert_matches::assert_matches;
    use async_trait::async_trait;
    use http::StatusCode;
    use http::{Request, Response};
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use mockall::mock;
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;
    use url::Url;

    mock! {
        #[derive(Debug)]
        pub OtelHttpClient {}
        #[async_trait]
        impl opentelemetry_http::HttpClient for OtelHttpClient {
            async fn send_bytes(&self, request:  Request<opentelemetry_http::Bytes>) -> Result<Response<opentelemetry_http::Bytes>, opentelemetry_http::HttpError>;
        }

        impl Clone for OtelHttpClient {
            fn clone(&self) -> Self;
        }
    }

    const INVALID_TESTING_CERT: &str =
        "-----BEGIN CERTIFICATE-----\ninvalid!\n-----END CERTIFICATE-----";

    fn valid_testing_cert() -> String {
        let subject_alt_names = vec!["localhost".to_string()];
        let rcgen::CertifiedKey {
            cert,
            signing_key: _,
        } = rcgen::generate_simple_self_signed(subject_alt_names).unwrap();
        cert.pem()
    }

    #[test]
    fn test_http_client_proxy() {
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
        // Build a http client using the proxy configuration
        let http_config = HttpConfig::new(
            Duration::from_secs(3),
            Duration::from_secs(3),
            ProxyConfig::from_url(proxy_server.base_url()),
        );
        let agent = HttpClient::new(http_config)
            .unwrap_or_else(|e| panic!("Unexpected error building the client {e}"));
        let resp = agent
            .client
            .get(target_server.url("/path").as_str())
            .send()
            .unwrap_or_else(|e| panic!("Error performing request: {e}"));
        // Check responses from the target server
        assert_eq!(resp.status(), StatusCode::OK.as_u16());
        assert_eq!(resp.text().unwrap(), expected_response.to_string())
    }

    #[test]
    fn test_certs_from_paths_no_certificates() {
        let ca_bundle_file = PathBuf::default();
        let ca_bundle_dir = PathBuf::default();
        let certificates = certs_from_paths(&ca_bundle_file, &ca_bundle_dir).unwrap();
        assert_eq!(certificates.len(), 0);
    }

    #[test]
    fn test_certs_from_paths_non_existing_certificate_path() {
        let ca_bundle_file = PathBuf::from("non-existing.pem");
        let ca_bundle_dir = PathBuf::default();
        let err = certs_from_paths(&ca_bundle_file, &ca_bundle_dir).unwrap_err();
        assert_matches!(err, HttpBuildError::CertificateError { .. });

        let ca_bundle_file = PathBuf::default();
        let ca_bundle_dir = PathBuf::from("non-existing-dir.pem");
        let err = certs_from_paths(&ca_bundle_file, &ca_bundle_dir).unwrap_err();
        assert_matches!(err, HttpBuildError::CertificateError { .. });
    }

    #[test]
    fn test_certs_from_paths_invalid_certificate_file() {
        let dir = tempdir().unwrap();
        let ca_bundle_file = dir.path().join("invalid_cert.pem");
        let mut file = File::create(&ca_bundle_file).unwrap();
        writeln!(file, "{INVALID_TESTING_CERT}").unwrap();

        let ca_bundle_dir = PathBuf::default();
        let err = certs_from_paths(&ca_bundle_file, &ca_bundle_dir).unwrap_err();
        assert_matches!(err, HttpBuildError::CertificateError { .. });
    }

    #[test]
    fn test_certs_from_paths_valid_certificate_file() {
        let dir = tempdir().unwrap();
        let ca_bundle_file = dir.path().join("valid_cert.pem");
        let mut file = File::create(&ca_bundle_file).unwrap();
        writeln!(file, "{}", valid_testing_cert()).unwrap();

        let ca_bundle_dir = PathBuf::default();
        let certificates = certs_from_paths(&ca_bundle_file, &ca_bundle_dir).unwrap();
        assert_eq!(certificates.len(), 1);
    }

    #[test]
    fn test_certs_from_paths_dir_poining_to_file() {
        let dir = tempdir().unwrap();
        let ca_bundle_dir = dir.path().join("valid_cert.pem");
        let mut file = File::create(&ca_bundle_dir).unwrap();
        writeln!(file, "{}", valid_testing_cert()).unwrap();

        let ca_bundle_file = PathBuf::default();
        let err = certs_from_paths(&ca_bundle_file, &ca_bundle_dir).unwrap_err();
        assert_matches!(err, HttpBuildError::CertificateError { .. });
    }

    #[test]
    fn test_certs_from_paths_valid_certificate_dir() {
        let dir = tempdir().unwrap();
        let ca_bundle_dir = dir.path();

        // Valid cert file
        let file_path = dir.path().join("valid_cert.pem");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "{}", valid_testing_cert()).unwrap();
        // Empty cert file
        let file_path = dir.path().join("empty_cert.pem");
        let _ = File::create(&file_path).unwrap();
        // Unrelated file
        let file_path = dir.path().join("other-file.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "some content").unwrap();
        // Invalid cert in no cert-file
        let file_path = dir.path().join("invalid-cert.bk");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "{INVALID_TESTING_CERT}").unwrap();

        let ca_bundle_file = PathBuf::default();
        let certificates = certs_from_paths(&ca_bundle_file, ca_bundle_dir).unwrap();
        assert_eq!(certificates.len(), 1);
    }

    #[test]
    fn test_error_conversions() {
        let http_err = HttpResponseError::UnsuccessfulResponse {
            status_code: StatusCode::UNAUTHORIZED,
            body: b"invalid token".to_vec(),
        };
        let oauth_err: OauthHttpClientError = http_err.into();
        assert_matches!(oauth_err, OauthHttpClientError::InvalidResponse(_));
        assert!(
            oauth_err
                .to_string()
                .contains("HTTP Error 401 Unauthorized")
        );

        let http_err = HttpResponseError::UnsuccessfulResponse {
            status_code: StatusCode::INTERNAL_SERVER_ERROR,
            body: b"server failed".to_vec(),
        };
        let opamp_err: OpampHttpClientError = http_err.into();
        assert_matches!(opamp_err, OpampHttpClientError::HTTPBodyError(_));
        assert!(
            opamp_err
                .to_string()
                .contains("HTTP Error 500 Internal Server Error")
        );

        let http_err = HttpResponseError::ReadingResponse("could not read".to_string());
        let opamp_err: OpampHttpClientError = http_err.into();
        assert_matches!(opamp_err, OpampHttpClientError::HTTPBodyError(_));
        assert!(opamp_err.to_string().contains("could not read"));
    }

    #[test]
    fn test_http_client_timeout() {
        let mock_server = MockServer::start();
        mock_server.mock(|when, then| {
            when.path("/");
            then.delay(Duration::from_millis(200)).status(200);
        });

        let http_config = HttpConfig::new(
            Duration::from_millis(50),
            Duration::from_millis(50),
            Default::default(),
        );
        let http_client = HttpClient::new(http_config).unwrap();

        let request = Request::builder()
            .uri(mock_server.url("/").as_str())
            .method("GET")
            .body(Vec::new())
            .unwrap();

        let result = http_client.send(request);
        assert_matches!(result, Err(HttpResponseError::TimeoutError(_)));
    }

    // This test seems to be testing the reqwest library, but it is useful to detect particular behaviors of the
    // underlying libraries. Context: some libraries, such as ureq, return an error if any response has a status code
    // not in the 2XX range and the client implementation needs to handle that properly.
    #[test]
    fn test_http_client() {
        struct TestCase {
            name: &'static str,
            status_code: u16,
            expects_success: bool,
        }

        impl TestCase {
            fn run(self) {
                let path = "/";
                let mock_server = MockServer::start();
                let mock = mock_server.mock(|when, then| {
                    when.path(path).method(GET);
                    then.status(self.status_code).body(self.name);
                });

                let http_config = HttpConfig::new(
                    Duration::from_secs(3),
                    Duration::from_secs(3),
                    Default::default(),
                );
                let url: Url = mock_server.url(path).parse().unwrap();
                let http_client = HttpClient::new(http_config).unwrap();

                let request = Request::builder()
                    .uri(url.as_str())
                    .method("GET")
                    .body(Vec::new())
                    .unwrap();

                let result = http_client.send(request);

                if self.expects_success {
                    let res = result.unwrap();
                    mock.assert();
                    assert_eq!(res.status().as_u16(), self.status_code);
                    assert_eq!(*res.body(), self.name.as_bytes());
                } else {
                    let err = result.unwrap_err();
                    mock.assert();
                    assert_matches!(err, HttpResponseError::UnsuccessfulResponse { .. });
                    if let HttpResponseError::UnsuccessfulResponse { status_code, body } = err {
                        assert_eq!(status_code.as_u16(), self.status_code);
                        assert_eq!(body, self.name.as_bytes());
                    }
                }
            }
        }
        let test_cases = [
            TestCase {
                name: "OK",
                status_code: 200,
                expects_success: true,
            },
            TestCase {
                name: "Not found",
                status_code: 404,
                expects_success: false,
            },
            TestCase {
                name: "Server error",
                status_code: 500,
                expects_success: false,
            },
        ];
        test_cases.into_iter().for_each(|tc| tc.run());
    }
}
