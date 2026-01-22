use crate::http::client::{cert_paths_from_dir, certificate_error};
use crate::http::config::ProxyConfig;
use crate::package::oci::artifact_definitions::LocalAgentPackage;
use crate::utils::retry::retry;
use oci_client::client::{Certificate, CertificateEncoding, ClientConfig};
use oci_client::errors::OciDistributionError;
use oci_client::{Client, secrets::RegistryAuth};
use oci_spec::distribution::Reference;
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio;
use tokio::runtime::Runtime;
use tracing::debug;
use url::Url;

#[derive(Debug, Error)]
pub enum OCIDownloaderError {
    #[error("downloading OCI artifact: {0}")]
    DownloadingArtifact(String),
    #[error("I/O error: {0}")]
    Io(std::io::Error),
    #[error("OCI error: {0}")]
    OciDistribution(OciDistributionError),
    #[error("certificate error: {0}")]
    Certificate(String),
}

/// An interface for downloading Agent Packages from an OCI registry.
pub trait OCIAgentDownloader: Send + Sync {
    fn download(
        &self,
        reference: &Reference,
        destination_dir: &Path,
    ) -> Result<LocalAgentPackage, OCIDownloaderError>;
}

// This is expected to be thread-safe since it is used in the package manager.
// Make sure that we are not writing to disk to the same location from multiple threads.
// This implementation avoids that since each download is expected to be done in a separate package directory.
pub struct OCIArtifactDownloader {
    client: Client,
    auth: RegistryAuth,
    runtime: Arc<Runtime>,
    max_retries: usize,
    retry_interval: Duration,
}

impl OCIAgentDownloader for OCIArtifactDownloader {
    /// Downloads an artifact from an OCI registry using a reference containing
    /// all the required data to first pull the image manifest if it exists and then iterate all the
    /// layers downloading each one and downloading the found package into a file where the name
    /// is the digest. Tokio file is used for async_write so the blob can be read in chunks.
    /// If retries are set up, it will retry downloading the artifact if it fails.
    ///
    /// Returns a vector of PathBufs where each path corresponds to a downloaded layer.
    fn download(
        &self,
        reference: &Reference,
        package_dir: &Path,
    ) -> Result<LocalAgentPackage, OCIDownloaderError> {
        debug!("Downloading '{reference}'",);
        retry(self.max_retries, self.retry_interval, || {
            self.runtime
                .block_on(self.download_package_artifact(reference, package_dir))
                .inspect_err(|e| debug!("Download '{reference}' failed with error: {e}"))
        })
        .map_err(|e| {
            OCIDownloaderError::DownloadingArtifact(format!(
                "Download attempts exceeded. Last error: {e}"
            ))
        })
    }
}

const DEFAULT_RETRIES: usize = 0;

impl OCIArtifactDownloader {
    /// try_new requires a package dir where the artifacts will be downloaded and a proxy_config
    /// that if url is empty will be ignored. By default, Auth is set to Anonymous, but it can be
    /// modified with the with_auth method.
    /// By default the number of retries is set to 0.
    pub fn try_new(
        proxy_config: ProxyConfig,
        runtime: Arc<Runtime>,
        client_config: ClientConfig,
    ) -> Result<Self, OCIDownloaderError> {
        let mut client_config = client_config;
        Self::proxy_setup(proxy_config, &mut client_config)?;

        Ok(OCIArtifactDownloader {
            client: Client::new(client_config),
            auth: RegistryAuth::Anonymous,
            runtime,
            max_retries: DEFAULT_RETRIES,
            retry_interval: Duration::default(),
        })
    }

    fn proxy_setup(
        proxy_config: ProxyConfig,
        client_config: &mut ClientConfig,
    ) -> Result<(), OCIDownloaderError> {
        let proxy_url = proxy_config.url_as_string();
        if !proxy_url.is_empty() {
            match Url::parse(&proxy_url).as_ref().map(Url::scheme) {
                Ok("http") => client_config.http_proxy = Some(proxy_url),
                Ok(_) | Err(_) => client_config.https_proxy = Some(proxy_url),
            };

            client_config.extra_root_certificates =
                certs_from_paths(proxy_config.ca_bundle_file(), proxy_config.ca_bundle_dir())
                    .map_err(|err| {
                        OCIDownloaderError::Certificate(format!("invalid cert file: {err}"))
                    })?;
        }

        Ok(())
    }

    pub fn with_auth(self, auth: RegistryAuth) -> Self {
        Self { auth, ..self }
    }

    pub fn with_retries(self, retries: usize, retry_interval: Duration) -> Self {
        Self {
            max_retries: retries,
            retry_interval,
            ..self
        }
    }

    async fn download_package_artifact(
        &self,
        reference: &Reference,
        package_dir: &Path,
    ) -> Result<LocalAgentPackage, OCIDownloaderError> {
        let (image_manifest, _) = self
            .client
            .pull_image_manifest(reference, &self.auth)
            .await
            .map_err(OCIDownloaderError::OciDistribution)?;

        let (layer, media_type) = LocalAgentPackage::get_layer(&image_manifest).map_err(|e| {
            OCIDownloaderError::DownloadingArtifact(format!("validating package layer: {e}"))
        })?;

        let layer_path = package_dir.join(layer.digest.replace(':', "_"));
        let mut file = tokio::fs::File::create(&layer_path)
            .await
            .map_err(OCIDownloaderError::Io)?;

        self.client
            .pull_blob(reference, &layer, &mut file)
            .await
            .map_err(OCIDownloaderError::OciDistribution)?;

        debug!("Artifact written to {}", layer_path.display());

        Ok(LocalAgentPackage::new(media_type, layer_path))
    }
}

/// Tries to extract certificates from the provided `ca_bundle_file` and `ca_bundle_dir` paths.
fn certs_from_paths(
    ca_bundle_file: &Path,
    ca_bundle_dir: &Path,
) -> Result<Vec<Certificate>, OCIDownloaderError> {
    let mut certs = Vec::new();
    // Certs from bundle file
    certs.extend(certs_from_file(ca_bundle_file)?);
    // Certs from bundle dir
    for path in cert_paths_from_dir(ca_bundle_dir)
        .map_err(|err| OCIDownloaderError::Certificate(err.to_string()))?
    {
        certs.extend(certs_from_file(&path)?)
    }
    Ok(certs)
}

/// Returns all certs bundled in the file corresponding to the provided path.
fn certs_from_file(path: &Path) -> Result<Vec<Certificate>, OCIDownloaderError> {
    if path.as_os_str().is_empty() {
        return Ok(Vec::new());
    }

    let file = File::open(path)
        .map_err(|err| OCIDownloaderError::Certificate(certificate_error(path, err).to_string()))?;
    let reader = BufReader::new(file);

    rustls_pki_types::CertificateDer::pem_reader_iter(reader).try_fold(Vec::default(), |acc, r| {
        match r {
            Err(_) => Err(OCIDownloaderError::Certificate(
                "invalid certificate encoding".to_string(),
            )),
            Ok(cert) => Ok(add_cert(acc, cert)),
        }
    })
}

fn add_cert<'a>(mut certs: Vec<Certificate>, cert: CertificateDer<'a>) -> Vec<Certificate> {
    certs.push(Certificate {
        encoding: CertificateEncoding::Pem,
        data: cert.as_ref().to_vec(),
    });
    certs
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use mockall::mock;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::tempdir;

    mock! {
        pub OCIDownloader {}
        impl OCIAgentDownloader for OCIDownloader {
            fn download(
                &self,
                reference: &Reference,
                package_dir: &Path,
            ) -> Result<LocalAgentPackage, OCIDownloaderError>;
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
    fn test_with_empty_proxy_url() {
        let proxy_config = ProxyConfig::from_url("".to_string()); // Assuming ProxyConfig::new method exists

        let mut client_config = ClientConfig::default();
        let proxy_result = OCIArtifactDownloader::proxy_setup(proxy_config, &mut client_config);
        assert!(proxy_result.is_ok());

        assert_eq!(client_config.https_proxy, None);
        assert_eq!(client_config.http_proxy, None);
    }

    #[test]
    fn test_valid_http_proxy_url() {
        let proxy_config = ProxyConfig::from_url("http://valid.proxy.url".to_string());

        let mut client_config = ClientConfig::default();
        let proxy_result = OCIArtifactDownloader::proxy_setup(proxy_config, &mut client_config);
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

        let proxy_config = ProxyConfig::new(
            "valid.proxy.url",
            Some(ca_bundle_dir.to_string_lossy().to_string()),
            None,
            false,
        );

        let mut client_config = ClientConfig::default();
        let proxy_result = OCIArtifactDownloader::proxy_setup(proxy_config, &mut client_config);
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
        let proxy_result = OCIArtifactDownloader::proxy_setup(proxy_config, &mut client_config);
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
        assert_matches!(err, OCIDownloaderError::Certificate { .. });

        let ca_bundle_file = PathBuf::default();
        let ca_bundle_dir = PathBuf::from("non-existing-dir.pem");
        let err = certs_from_paths(&ca_bundle_file, &ca_bundle_dir).unwrap_err();
        assert_matches!(err, OCIDownloaderError::Certificate { .. });
    }

    #[test]
    fn test_certs_from_paths_invalid_certificate_file() {
        let dir = tempdir().unwrap();
        let ca_bundle_file = dir.path().join("invalid_cert.pem");
        let mut file = File::create(&ca_bundle_file).unwrap();
        writeln!(file, "{INVALID_TESTING_CERT}").unwrap();

        let ca_bundle_dir = PathBuf::default();
        let err = certs_from_paths(&ca_bundle_file, &ca_bundle_dir).unwrap_err();
        assert_matches!(err, OCIDownloaderError::Certificate { .. });
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
        assert_matches!(err, OCIDownloaderError::Certificate { .. });
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
