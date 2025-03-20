//! # Helpers to build a reqwest blocking client and handle responses and handle responses
use crate::http::config::HttpConfig;
use http::Response as HttpResponse;
use http::{Request, Response};
use nix::NixPath;
use nr_auth::http_client::HttpClient as OauthHttpClient;
use nr_auth::http_client::HttpClientError as OauthHttpClientError;
use opamp_client::http::HttpClientError as OpampHttpClientError;
use reqwest::tls::TlsInfo;
use reqwest::{
    blocking::{Client, Response as BlockingResponse},
    Certificate, Proxy,
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
        let req = self
            .client
            .request(request.method().into(), request.uri().to_string().as_str())
            .headers(request.headers().clone())
            .body(request.body().to_vec());

        let res = req
            .send()
            .map_err(|err| HttpResponseError::TransportError(err.to_string()))?;

        try_build_response(res)
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
    #[error("`{0}`")]
    TransportError(String),
}

impl From<HttpResponseError> for OpampHttpClientError {
    fn from(err: HttpResponseError) -> Self {
        match err {
            HttpResponseError::TransportError(msg) => OpampHttpClientError::TransportError(msg),
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
        CloudClientError::TransportError(err.to_string())
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
            HttpResponseError::TransportError(msg) => OauthHttpClientError::TransportError(msg),
            HttpResponseError::BuildingRequest(msg)
            | HttpResponseError::BuildingResponse(msg)
            | HttpResponseError::ReadingResponse(msg) => OauthHttpClientError::InvalidResponse(msg),
        }
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
    if path.is_empty() {
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
    if dir_path.is_empty() {
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
mod tests {
    use crate::http::config::ProxyConfig;

    use super::*;
    use assert_matches::assert_matches;
    use http::StatusCode;
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;
    use url::Url;

    const INVALID_TESTING_CERT: &str =
        "-----BEGIN CERTIFICATE-----\ninvalid!\n-----END CERTIFICATE-----";

    fn valid_testing_cert() -> String {
        let subject_alt_names = vec!["localhost".to_string()];
        let rcgen::CertifiedKey { cert, key_pair: _ } =
            rcgen::generate_simple_self_signed(subject_alt_names).unwrap();
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

    // This test seems to be testing the reqwest library, but it is useful to detect particular behaviors of the
    // underlying libraries. Context: some libraries, such as ureq, return an error if any response has a status code
    // not in the 2XX range and the client implementation needs to handle that properly.
    #[test]
    fn test_http_client() {
        struct TestCase {
            name: &'static str,
            status_code: u16,
        }

        impl TestCase {
            fn run(self) {
                let path = "/";
                let mock_server = MockServer::start();
                let req_mock = mock_server.mock(|when, then| {
                    when.path(path).method(GET);
                    then.status(self.status_code).body(self.name);
                });

                let http_config = HttpConfig::new(
                    Duration::from_secs(3),
                    Duration::from_secs(3),
                    Default::default(),
                );
                let url: Url = mock_server.url(path).parse().unwrap_or_else(|err| {
                    panic!(
                        "could not parse the mock-server url: {} - {}",
                        err, self.name
                    )
                });
                let http_client = HttpClient::new(http_config).unwrap_or_else(|err| {
                    panic!(
                        "unexpected error building the http client {} - {}",
                        err, self.name
                    )
                });

                let request = Request::builder()
                    .uri(url.as_str())
                    .method("GET")
                    .body(Vec::new())
                    .unwrap();

                let res = http_client.send(request).unwrap();

                req_mock.assert_calls(1);
                assert_eq!(
                    res.status(),
                    self.status_code,
                    "not expected status code in {}",
                    self.name
                );
                assert_eq!(
                    *res.body(),
                    self.name.to_string().as_bytes().to_vec(),
                    "not expected body code in {}",
                    self.name
                );
            }
        }
        let test_cases = [
            TestCase {
                name: "OK",
                status_code: 200,
            },
            TestCase {
                name: "Not found",
                status_code: 404,
            },
            TestCase {
                name: "Server error",
                status_code: 500,
            },
        ];
        test_cases.into_iter().for_each(|tc| tc.run());
    }
}
