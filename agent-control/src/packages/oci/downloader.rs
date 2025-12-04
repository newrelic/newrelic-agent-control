use crate::http::client::{cert_paths_from_dir, certificate_error};
use crate::http::config::ProxyConfig;
use oci_client::client::{Certificate, CertificateEncoding, ClientConfig};
use oci_client::{Client, secrets::RegistryAuth};
use oci_spec::distribution::Reference;
use rustls_pki_types::pem::PemObject;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio;
use tracing::debug;
use url::Url;

#[derive(Debug, Error)]
pub enum OCIDownloaderError {
    #[error("donwnloading OCI artifact: {0}")]
    DownloadingArtifactError(String),
    #[error("certificate error: {0}")]
    CertificateError(String),
}

pub struct OCIDownloader {
    client: Client,
    auth: RegistryAuth,
}

#[allow(dead_code, reason = "still unused")]
impl OCIDownloader {
    /// try_new requires a package dir where the artifacts will be downloaded and a proxy_config
    /// that if url is empty will be ignored. By default, Auth is set to Anonymous, but it can be
    /// modified with the with_auth method.
    pub fn try_new(proxy_config: ProxyConfig) -> Result<Self, OCIDownloaderError> {
        let mut client_config = ClientConfig::default();
        Self::proxy_setup(proxy_config, &mut client_config)?;

        Ok(OCIDownloader {
            client: Client::new(client_config),
            auth: RegistryAuth::Anonymous,
        })
    }

    fn proxy_setup(
        proxy_config: ProxyConfig,
        client_config: &mut ClientConfig,
    ) -> Result<(), OCIDownloaderError> {
        let proxy_url = proxy_config.url_as_string();
        if !proxy_url.is_empty() {
            let scheme = Url::parse(&proxy_url)
                .map(|url| match url.scheme() {
                    "http" | "https" => url.scheme().to_string(),
                    _ => "https".to_string(),
                })
                .unwrap_or_else(|_| "https".to_string());

            if scheme == "http" {
                client_config.http_proxy = Some(proxy_url);
            } else {
                client_config.https_proxy = Some(proxy_url);
            }

            client_config.extra_root_certificates =
                certs_from_paths(proxy_config.ca_bundle_file(), proxy_config.ca_bundle_dir())
                    .map_err(|err| {
                        OCIDownloaderError::CertificateError(format!("invalid cert file: {err}"))
                    })?;
        }
        Ok(())
    }

    pub fn with_auth(self, auth: RegistryAuth) -> Self {
        Self { auth, ..self }
    }

    /// download_artifact downloads an artifact from the oci registry using a reference containing
    /// all the required data to first pull the image manifest if it exists and then iterate all the
    /// layers downloading each one and downloading the found package into a file where the name
    /// is the digest. Tokio file is used for async_write so the blob can be read in chunks.
    pub async fn download_artifact(
        &self,
        reference: Reference,
        package_dir: PathBuf,
    ) -> Result<(), OCIDownloaderError> {
        let (image_manifest, _) = self
            .client
            .pull_image_manifest(&reference, &self.auth)
            .await
            .map_err(|err| {
                OCIDownloaderError::DownloadingArtifactError(format!(
                    "Failed to download OCI manifest: {}",
                    err
                ))
            })?;
        for layer in image_manifest.layers.iter() {
            let layer_path = package_dir.join(layer.digest.clone());
            let mut file = tokio::fs::File::create(&layer_path).await.map_err(|err| {
                OCIDownloaderError::DownloadingArtifactError(format!(
                    "Failed to create OCI artifact file: {}",
                    err
                ))
            })?;
            self.client
                .pull_blob(&reference, &layer, &mut file)
                .await
                .map_err(|err| {
                    OCIDownloaderError::DownloadingArtifactError(format!(
                        "Failed pulling OCI blob into artifact file: {}",
                        err
                    ))
                })?;
            debug!("Artifact written to {}", layer_path.to_string_lossy());
        }

        Ok(())
    }
}

/// Tries to extract certificates from the provided `ca_bundle_file` and `ca_bundle_dir` paths.
#[allow(dead_code, reason = "still unused")]
fn certs_from_paths(
    ca_bundle_file: &Path,
    ca_bundle_dir: &Path,
) -> Result<Vec<Certificate>, OCIDownloaderError> {
    let mut certs = Vec::new();
    // Certs from bundle file
    certs.extend(certs_from_file(ca_bundle_file)?);
    // Certs from bundle dir
    for path in cert_paths_from_dir(ca_bundle_dir)
        .map_err(|err| OCIDownloaderError::CertificateError(err.to_string()))?
    {
        certs.extend(certs_from_file(&path)?)
    }
    Ok(certs)
}

/// Returns all certs bundled in the file corresponding to the provided path.
#[allow(dead_code, reason = "still unused")]
fn certs_from_file(path: &Path) -> Result<Vec<Certificate>, OCIDownloaderError> {
    if path.as_os_str().is_empty() {
        return Ok(Vec::new());
    }

    let file = File::open(path).map_err(|err| {
        OCIDownloaderError::CertificateError(certificate_error(path, err).to_string())
    })?;
    let reader = BufReader::new(file);

    let certificates: Result<Vec<Vec<u8>>, OCIDownloaderError> =
        rustls_pki_types::CertificateDer::pem_reader_iter(reader)
            .map(|result| {
                result.map(|cert| cert.as_ref().to_vec()).map_err(|_| {
                    OCIDownloaderError::CertificateError("invalid certificate encoding".to_string())
                })
            })
            .collect();

    let certs: Vec<Certificate> = certificates?
        .into_iter()
        .map(|data| Certificate {
            encoding: CertificateEncoding::Pem,
            data,
        })
        .collect();

    Ok(certs)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::tempdir;

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
    fn test_with_empty_proxy_url() {
        let proxy_config = ProxyConfig::from_url("".to_string()); // Assuming ProxyConfig::new method exists

        let mut client_config = ClientConfig::default();
        let proxy_result = OCIDownloader::proxy_setup(proxy_config, &mut client_config);
        assert!(proxy_result.is_ok());

        assert_eq!(client_config.https_proxy, None);
        assert_eq!(client_config.http_proxy, None);
    }

    #[test]
    fn test_valid_http_proxy_url() {
        let proxy_config = ProxyConfig::from_url("http://valid.proxy.url".to_string());

        let mut client_config = ClientConfig::default();
        let proxy_result = OCIDownloader::proxy_setup(proxy_config, &mut client_config);
        assert!(proxy_result.is_ok());

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

        let proxy_config = crate::cli::on_host::config_gen::config::ProxyConfig {
            proxy_url: Some("valid.proxy.url".to_string()),
            proxy_ca_bundle_dir: Some(ca_bundle_dir.to_str().unwrap().to_string()),
            proxy_ca_bundle_file: None,
            ignore_system_proxy: false,
        };

        let proxy_config_parsed = ProxyConfig::try_from(proxy_config).unwrap();

        let mut client_config = ClientConfig::default();
        let proxy_result = OCIDownloader::proxy_setup(proxy_config_parsed, &mut client_config);
        assert!(proxy_result.is_ok());

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

        let mut client_config = ClientConfig::default();
        let proxy_result = OCIDownloader::proxy_setup(proxy_config, &mut client_config);
        assert!(proxy_result.is_ok());

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
        assert_matches!(err, OCIDownloaderError::CertificateError { .. });

        let ca_bundle_file = PathBuf::default();
        let ca_bundle_dir = PathBuf::from("non-existing-dir.pem");
        let err = certs_from_paths(&ca_bundle_file, &ca_bundle_dir).unwrap_err();
        assert_matches!(err, OCIDownloaderError::CertificateError { .. });
    }

    #[test]
    fn test_certs_from_paths_invalid_certificate_file() {
        let dir = tempdir().unwrap();
        let ca_bundle_file = dir.path().join("invalid_cert.pem");
        let mut file = File::create(&ca_bundle_file).unwrap();
        writeln!(file, "{INVALID_TESTING_CERT}").unwrap();

        let ca_bundle_dir = PathBuf::default();
        let err = certs_from_paths(&ca_bundle_file, &ca_bundle_dir).unwrap_err();
        assert_matches!(err, OCIDownloaderError::CertificateError { .. });
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
        assert_matches!(err, OCIDownloaderError::CertificateError { .. });
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
