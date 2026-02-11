//! This module defines helpers for setting up the proxy configuration in the OCI client.

use std::{fs::File, io::BufReader, path::Path};

use http::uri::Scheme;
use oci_client::client::{Certificate, CertificateEncoding, ClientConfig};
use rustls_pki_types::pem::PemObject;

use crate::http::{
    client::{HttpBuildError, cert_paths_from_dir, certificate_error},
    config::ProxyConfig,
};

use super::OciClientError;

/// Returns a [ClientConfig] corresponding to `client_config` with the provided `proxy_config` applied.
pub(super) fn setup_proxy(
    client_config: ClientConfig,
    proxy_config: ProxyConfig,
) -> Result<ClientConfig, OciClientError> {
    let mut config = client_config;
    if let Some(uri) = proxy_config.url() {
        if uri.scheme() == Some(&Scheme::HTTP) {
            config.http_proxy = Some(proxy_config.url_as_string());
        } else {
            config.https_proxy = Some(proxy_config.url_as_string());
        }

        config.extra_root_certificates =
            certs_from_paths(proxy_config.ca_bundle_file(), proxy_config.ca_bundle_dir())
                .map_err(|err| OciClientError::Build(err.to_string()))?;
    }
    Ok(config)
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

    let file = File::open(path).map_err(|err| certificate_error(path, err))?;
    let reader = BufReader::new(file);
    rustls_pki_types::CertificateDer::pem_reader_iter(reader)
        .map(|cert| {
            cert.map(|cert| Certificate {
                encoding: CertificateEncoding::Pem,
                data: cert.as_ref().to_vec(),
            })
        })
        .collect::<Result<Vec<Certificate>, _>>()
        .map_err(|err| certificate_error(path, format!("invalid certificate encoding: {err}")))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;

    use assert_matches::assert_matches;
    use tempfile::tempdir;

    use super::*;
    #[test]
    fn test_with_empty_proxy_url() {
        let proxy_config = ProxyConfig::from_url("".to_string());

        let client_config = setup_proxy(ClientConfig::default(), proxy_config).unwrap();

        assert_eq!(client_config.https_proxy, None);
        assert_eq!(client_config.http_proxy, None);
    }

    #[test]
    fn test_valid_http_proxy_url() {
        let proxy_config = ProxyConfig::from_url("http://valid.proxy.url".to_string());

        let client_config = setup_proxy(ClientConfig::default(), proxy_config).unwrap();

        assert_eq!(client_config.https_proxy, None);
        assert_eq!(
            client_config.http_proxy,
            Some("http://valid.proxy.url/".to_string())
        );
    }

    #[test]
    fn test_proxy_url_without_scheme_with_certs() {
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

        let proxy_config = ProxyConfig::new(
            "valid.proxy.url",
            Some(ca_bundle_dir.to_string_lossy().to_string()),
            None,
            false,
        );

        let client_config = setup_proxy(ClientConfig::default(), proxy_config).unwrap();

        assert_eq!(
            client_config.https_proxy,
            Some("valid.proxy.url".to_string())
        );
        assert_eq!(client_config.http_proxy, None);
        assert_eq!(client_config.extra_root_certificates.len(), 1);
    }

    #[test]
    fn test_try_new_with_https_proxy_url() {
        let proxy_config = ProxyConfig::from_url("https://valid.proxy.url".to_string());

        let client_config = setup_proxy(ClientConfig::default(), proxy_config).unwrap();

        assert_eq!(
            client_config.https_proxy,
            Some("https://valid.proxy.url/".to_string())
        );
        assert_eq!(client_config.http_proxy, None);
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
    fn test_certs_from_paths_dir_pointing_to_file() {
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
}
