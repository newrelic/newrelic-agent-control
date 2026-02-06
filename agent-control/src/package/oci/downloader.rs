use crate::oci::{Client, OciClientError};
use crate::package::oci::artifact_definitions::LocalAgentPackage;
use crate::utils::retry::retry;
use oci_spec::distribution::Reference;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio;
use tokio::runtime::Runtime;
use tracing::debug;

#[derive(Debug, Error)]
pub enum OCIDownloaderError {
    #[error("downloading OCI artifact: {0}")]
    DownloadingArtifact(String),
    #[error("I/O error: {0}")]
    Io(std::io::Error),
    #[error("failure on OCI client request: {0}")]
    Client(OciClientError),
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
                "download attempts exceeded. Last error: {e}"
            ))
        })
    }
}

const DEFAULT_RETRIES: usize = 0;

impl OCIArtifactDownloader {
    /// Returns an artifact downloader with default retries setup.
    pub fn new(client: Client, runtime: Arc<Runtime>) -> Self {
        OCIArtifactDownloader {
            client,
            runtime,
            max_retries: DEFAULT_RETRIES,
            retry_interval: Duration::default(),
        }
    }

    /// Returns a new downloader with the provided retry configuration.
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
            .pull_image_manifest(reference)
            .await
            .map_err(OCIDownloaderError::Client)?;

        let (layer, media_type) = LocalAgentPackage::get_layer(&image_manifest).map_err(|e| {
            OCIDownloaderError::DownloadingArtifact(format!("validating package manifest: {e}"))
        })?;

        let layer_path = package_dir.join(layer.digest.replace(':', "_"));
        let mut file = tokio::fs::File::create(&layer_path)
            .await
            .map_err(OCIDownloaderError::Io)?;

        self.client
            .pull_blob(reference, &layer, &mut file)
            .await
            .map_err(OCIDownloaderError::Client)?;

        // Ensure all data is flushed to disk before returning
        file.sync_data().await.map_err(OCIDownloaderError::Io)?;

        debug!("Artifact written to {}", layer_path.display());

        Ok(LocalAgentPackage::new(media_type, layer_path))
    }
}

#[cfg(test)]
pub mod tests {
    use crate::http::config::ProxyConfig;
    use crate::oci::tests::FakeOciServer;
    use crate::package::oci::artifact_definitions::{
        LayerMediaType, ManifestArtifactType, PackageMediaType,
    };

    use super::*;
    use httpmock::prelude::*;
    use mockall::mock;
    use oci_client::client::{ClientConfig, ClientProtocol};
    use oci_spec::distribution::Reference;
    use serde_json::json;
    use std::str::FromStr;
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

    #[test]
    fn test_download_agent_package_success() {
        let server = FakeOciServer::new("test-repo", "v1.0.0")
            .with_artifact_type(&ManifestArtifactType::AgentPackage.to_string())
            .with_layer(
                b"test agent package content",
                &LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerTarGz).to_string(),
            )
            .build();

        let downloader = create_downloader();
        let dest_dir = tempdir().unwrap();
        let local_agent_package = downloader
            .download(&server.reference(), dest_dir.path())
            .unwrap();

        assert_eq!(
            std::fs::read(local_agent_package.path()).unwrap(),
            b"test agent package content"
        );
    }

    #[test]
    fn test_download_with_multiple_layers() {
        let server = FakeOciServer::new("test-repo", "v1.0.0")
            .with_artifact_type(&ManifestArtifactType::AgentPackage.to_string())
            .with_layer(
                b"layer 1 content",
                &LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerTarGz).to_string(),
            )
            .with_layer(
                b"layer 2 content",
                "application/vnd.newrelic.agent.unknown-content.v1",
            )
            .build();

        let downloader = create_downloader();
        let dest_dir = tempdir().unwrap();
        let local_agent_package = downloader
            .download(&server.reference(), dest_dir.path())
            .unwrap();

        assert_eq!(
            std::fs::read(local_agent_package.path()).unwrap(),
            b"layer 1 content"
        );
    }

    #[test]
    fn test_download_with_invalid_package() {
        let server = FakeOciServer::new("test-repo", "v1.0.0")
            .with_layer(
                b"test content",
                &LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerTarGz).to_string(),
            )
            .with_artifact_type("application/vnd.unknown.type.v1")
            .build();

        let downloader = create_downloader();
        let dest_dir = tempdir().unwrap();
        let err = downloader
            .download(&server.reference(), dest_dir.path())
            .unwrap_err();
        assert!(err.to_string().contains("validating package manifest"));
    }

    #[test]
    fn test_download_with_missing_manifest() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/v2/test-repo/manifests/v1.0.0");
            then.status(404).json_body(json!({
                "errors": [{"code": "MANIFEST_UNKNOWN", "message": "manifest unknown"}]
            }));
        });

        let reference =
            Reference::from_str(&format!("{}/test-repo:v1.0.0", server.address())).unwrap();
        let downloader = create_downloader();
        let dest_dir = tempdir().unwrap();
        let err = downloader
            .download(&reference, dest_dir.path())
            .unwrap_err();
        assert!(
            err.to_string().contains("download attempts exceeded"),
            "{}",
            err.to_string()
        );
    }

    fn create_downloader() -> OCIArtifactDownloader {
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let client = Client::try_new(
            ProxyConfig::default(),
            ClientConfig {
                protocol: ClientProtocol::Http,
                ..Default::default()
            },
        )
        .unwrap();
        OCIArtifactDownloader::new(client, runtime)
    }
}
