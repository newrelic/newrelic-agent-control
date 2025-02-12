//! # Helpers to build a reqwest blocking client and handle responses and handle responses
use super::config::HttpConfig;
use nix::NixPath;
use reqwest::{
    blocking::{Client, ClientBuilder, Response},
    Certificate, Proxy,
};
use std::{
    fmt::Display,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};
use tracing::warn;

const CERT_EXTENSION: &str = "pem";

#[derive(thiserror::Error, Debug)]
pub enum ReqwestResponseError {
    #[error("could read response body: {0}")]
    ReadingResponse(String),
    #[error("could build response: {0}")]
    BuildingResponse(String),
}

impl From<ReqwestResponseError> for nr_auth::http_client::HttpClientError {
    fn from(err: ReqwestResponseError) -> Self {
        Self::InvalidResponse(err.to_string())
    }
}

impl From<ReqwestResponseError> for opamp_client::http::HttpClientError {
    fn from(err: ReqwestResponseError) -> Self {
        Self::HTTPBodyError(err.to_string())
    }
}

/// Helper to build a [http::Response<Vec<u8>>] from a reqwest's blocking response.
/// It includes status, version and body. Headers are not included but they could be added if needed.
pub fn try_build_response(res: Response) -> Result<http::Response<Vec<u8>>, ReqwestResponseError> {
    let status = res.status();
    let version = res.version();
    let body: Vec<u8> = res
        .bytes()
        .map_err(|err| ReqwestResponseError::ReadingResponse(err.to_string()))?
        .into();
    http::Response::builder()
        .status(status)
        .version(version)
        .body(body)
        .map_err(|err| ReqwestResponseError::BuildingResponse(err.to_string()))
}

#[derive(thiserror::Error, Debug)]
pub enum ReqwestBuildError {
    #[error("could not build the reqwest client: {0}")]
    ClientBuilder(String),
    #[error("could not load certificates from {path}: {err}")]
    CertificateError { path: String, err: String },
}

/// Builds a reqwest blocking client according to the provided configuration.
pub fn try_build_reqwest_client(config: HttpConfig) -> Result<Client, ReqwestBuildError> {
    let mut builder = reqwest_builder_with_timeout(config.timeout, config.conn_timeout);

    let proxy_config = config.proxy;
    let proxy_url = proxy_config.url_as_string();
    if !proxy_url.is_empty() {
        let proxy = Proxy::all(proxy_url)
            .map_err(|err| ReqwestBuildError::ClientBuilder(format!("invalid proxy url: {err}")))?;
        builder = builder.proxy(proxy);
        for cert in certs_from_paths(proxy_config.ca_bundle_file(), proxy_config.ca_bundle_dir())? {
            builder = builder.add_root_certificate(cert)
        }
    }

    let client = builder
        .build()
        .map_err(|err| ReqwestBuildError::ClientBuilder(err.to_string()))?;
    Ok(client)
}

/// Returns a reqwest [ClientBuilder] with the default setup for Agent Control and the provider timeout values.
pub fn reqwest_builder_with_timeout(timeout: Duration, conn_timeout: Duration) -> ClientBuilder {
    Client::builder()
        .use_rustls_tls() // Use rust-tls backend
        .tls_built_in_native_certs(true) // Load system (native) certificates
        .timeout(timeout)
        .connect_timeout(conn_timeout)
}

/// Tries to extract certificates from the provided `ca_bundle_file` and `ca_bundle_dir` paths.
fn certs_from_paths(
    ca_bundle_file: &Path,
    ca_bundle_dir: &Path,
) -> Result<Vec<Certificate>, ReqwestBuildError> {
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
fn certs_from_file(path: &Path) -> Result<Vec<Certificate>, ReqwestBuildError> {
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
fn cert_paths_from_dir(dir_path: &Path) -> Result<Vec<PathBuf>, ReqwestBuildError> {
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

/// Helper to build a [ReqwestBuildError::CertificateError] more concisely.
fn certificate_error<E: Display>(path: &Path, err: E) -> ReqwestBuildError {
    ReqwestBuildError::CertificateError {
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
    use crate::http::proxy::ProxyConfig;

    use super::*;
    use assert_matches::assert_matches;
    use http::StatusCode;
    use httpmock::MockServer;
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;

    const INVALID_TESTING_CERT: &str =
        "-----BEGIN CERTIFICATE-----\ninvalid!\n-----END CERTIFICATE-----";

    fn valid_testing_cert() -> String {
        let subject_alt_names = vec!["localhost".to_string()];
        let rcgen::CertifiedKey { cert, key_pair: _ } =
            rcgen::generate_simple_self_signed(subject_alt_names).unwrap();
        cert.pem()
    }

    #[test]
    fn test_reqwest_proxy() {
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
        // Build a reqwest client using the proxy configuration
        let config = HttpConfig::new(
            Duration::from_secs(3),
            Duration::from_secs(3),
            ProxyConfig::from_url(proxy_server.base_url()),
        );
        let agent = try_build_reqwest_client(config)
            .unwrap_or_else(|e| panic!("Unexpected error building the client {e}"));
        let resp = agent
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
        assert_matches!(err, ReqwestBuildError::CertificateError { .. });

        let ca_bundle_file = PathBuf::default();
        let ca_bundle_dir = PathBuf::from("non-existing-dir.pem");
        let err = certs_from_paths(&ca_bundle_file, &ca_bundle_dir).unwrap_err();
        assert_matches!(err, ReqwestBuildError::CertificateError { .. });
    }

    #[test]
    fn test_certs_from_paths_invalid_certificate_file() {
        let dir = tempdir().unwrap();
        let ca_bundle_file = dir.path().join("invalid_cert.pem");
        let mut file = File::create(&ca_bundle_file).unwrap();
        writeln!(file, "{INVALID_TESTING_CERT}").unwrap();

        let ca_bundle_dir = PathBuf::default();
        let err = certs_from_paths(&ca_bundle_file, &ca_bundle_dir).unwrap_err();
        assert_matches!(err, ReqwestBuildError::CertificateError { .. });
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
        assert_matches!(err, ReqwestBuildError::CertificateError { .. });
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
}
