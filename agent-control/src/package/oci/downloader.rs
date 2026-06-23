use crate::agent_control::config::OciAuth;
use crate::agent_control::config::Registry;
use crate::oci::artifact_definitions::LocalAgentPackage;
use crate::oci::{Client, OciArtifactFetcher, OciClientError};
use crate::package::manager::PackageData;
use oci_client::Reference;
use oci_client::secrets::RegistryAuth;
use std::path::Path;
use std::time::Duration;
use tracing::{debug, warn};
use url::Url;

/// An interface for downloading Agent Packages from an OCI registry.
pub trait OCIPackageDownloader: Send + Sync {
    fn download(
        &self,
        package_data: &PackageData,
        destination_dir: &Path,
    ) -> Result<LocalAgentPackage, OciClientError>;
}

// This is expected to be thread-safe since it is used in the package manager.
// Make sure that we are not writing to disk to the same location from multiple threads.
// This implementation avoids that since each download is expected to be done in a separate package directory.
pub struct OCIPackageArtifactDownloader {
    fetcher: OciArtifactFetcher,
    signature_verification_enabled: bool,
    registry: Registry,
}

impl OCIPackageDownloader for OCIPackageArtifactDownloader {
    /// Download the artifact contained in the provided `package_data` and store its content to `package_dir`.
    ///
    /// If signature verification is enabled and a public key url is provided in `package_data`, it first verifies the artifact's
    /// signature and then downloads the artifact that has verified (the verified reference is identified by `digest`
    /// in order to assure that the artifact downloaded is the one that has been verified).
    ///
    /// In case of failure, the download operation is retried as configured in the downloader and if all download
    /// attempts are reached, it returns an error. If download succeeds, it returns the corresponding
    /// [LocalAgentPackage] containing the package information.
    fn download(
        &self,
        package_data: &PackageData,
        package_dir: &Path,
    ) -> Result<LocalAgentPackage, OciClientError> {
        debug!(
            "Downloading from repository '{}' with version '{}'",
            package_data.oci.repository, package_data.oci.version
        );
        let base_reference = package_data
            .oci
            .to_reference(&self.registry)
            .map_err(|e| OciClientError::FetchArtifact(format!("building OCI reference: {e}")))?;
        let public_key_url = self.should_verify_signature(&package_data.oci.public_key_url);
        self.fetcher.fetch(
            &base_reference,
            public_key_url,
            |client, reference, auth| {
                Self::download_package_artifact(client, reference, auth, package_dir)
            },
        )
    }
}

impl OCIPackageArtifactDownloader {
    /// Returns an artifact downloader with default retries setup.
    pub fn new(
        client: Client,
        registry: Registry,
        auth: Option<OciAuth>,
        signature_verification_enabled: bool,
    ) -> Self {
        OCIPackageArtifactDownloader {
            fetcher: OciArtifactFetcher::new(client, auth),
            signature_verification_enabled,
            registry,
        }
    }

    /// Returns a new downloader with the provided retry configuration.
    pub fn with_retries(self, retries: usize, retry_interval: Duration) -> Self {
        Self {
            fetcher: self.fetcher.with_retries(retries, retry_interval),
            ..self
        }
    }

    /// This helper returns the `public_key_url` if signature verification needs to be performed, None otherwise
    fn should_verify_signature<'a>(&self, public_key_url: &'a Option<Url>) -> Option<&'a Url> {
        if !self.signature_verification_enabled {
            warn!("Signature verification is disabled, skipping");
            return None;
        }
        let Some(pk_url) = public_key_url else {
            warn!("No public_key_url for agent package, skipping signature verification");
            return None;
        };
        Some(pk_url)
    }

    /// Pulls the manifest, validates it is an agent package artifact and pulls its layer to a file
    /// under `package_dir`, returning the resulting [LocalAgentPackage].
    fn download_package_artifact(
        client: &Client,
        reference: &Reference,
        auth: &RegistryAuth,
        package_dir: &Path,
    ) -> Result<LocalAgentPackage, OciClientError> {
        let (image_manifest, _) = client.pull_image_manifest(reference, auth).map_err(|err| {
            OciClientError::FetchArtifact(format!("downloading package manifest: {err}"))
        })?;

        let (layer, media_type) = LocalAgentPackage::get_layer(&image_manifest).map_err(|err| {
            OciClientError::FetchArtifact(format!("validating package manifest: {err}"))
        })?;

        let layer_path = package_dir.join(layer.digest.replace(':', "_"));
        client
            .pull_blob_to_file(reference, &layer, &layer_path)
            .map_err(|err| {
                OciClientError::FetchArtifact(format!("downloading package artifact: {err}"))
            })?;

        debug!("Artifact written to {}", layer_path.display());

        Ok(LocalAgentPackage::new(media_type, layer_path))
    }
}

#[cfg(test)]
pub mod tests {
    use std::str::FromStr;

    use crate::agent_type::runtime_config::on_host::package::rendered::{Oci, Repository, Version};
    use crate::http::config::ProxyConfig;

    use crate::oci::artifact_definitions::{
        LayerMediaType, ManifestArtifactType, PackageMediaType,
    };
    use crate::oci::tests::FakeOciServer;
    use crate::signature::public_key::tests::TestKeyPair;
    use crate::signature::public_key_fetcher::tests::JwksMockServer;
    use crate::utils::test_runtime::tokio_runtime;

    use super::*;
    use mockall::mock;

    use oci_client::client::{ClientConfig, ClientProtocol};

    use tempfile::tempdir;

    mock! {
        pub OCIDownloader {}
        impl OCIPackageDownloader for OCIDownloader {
            fn download(
                &self,
                package_data: &PackageData,
                package_dir: &Path,
            ) -> Result<LocalAgentPackage, OciClientError>;
        }
    }

    const REPOSITORY: &str = "test-repo";
    const VERSION: &str = "v1.0.0";

    fn test_package_data(public_key_url: Option<Url>) -> PackageData {
        PackageData {
            id: "test-package".to_string(),
            oci: Oci {
                repository: Repository::from_str(REPOSITORY).unwrap(),
                version: Version::from_str(VERSION).unwrap(),
                public_key_url,
            },
            post_download_hook: None,
        }
    }

    #[test]
    fn test_download_agent_package_success() {
        let key_pair = TestKeyPair::new(0);
        let jwks_server = JwksMockServer::new(vec![
            serde_json::to_value(key_pair.public_key_jwk()).unwrap(),
        ]);
        let server = FakeOciServer::new(REPOSITORY, VERSION)
            .with_artifact_type(&ManifestArtifactType::AgentPackage.to_string())
            .with_layer(
                b"test agent package content",
                &LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerTarGz).to_string(),
            )
            .with_signature(&key_pair)
            .build();

        let downloader = create_downloader(server.registry(), true);
        let package_data = test_package_data(Some(jwks_server.url));
        let dest_dir = tempdir().unwrap();
        let local_agent_package = downloader.download(&package_data, dest_dir.path()).unwrap();

        assert_eq!(
            std::fs::read(local_agent_package.path()).unwrap(),
            b"test agent package content"
        );
    }

    #[test]
    fn test_download_agent_package_success_signature_verification_disabled_and_unsigned_artifact() {
        let key_pair = TestKeyPair::new(0);
        let jwks_server = JwksMockServer::new(vec![
            serde_json::to_value(key_pair.public_key_jwk()).unwrap(),
        ]);
        let server = FakeOciServer::new(REPOSITORY, VERSION)
            .with_artifact_type(&ManifestArtifactType::AgentPackage.to_string())
            .with_layer(
                b"test agent package content",
                &LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerTarGz).to_string(),
            )
            .build();

        let downloader = create_downloader(server.registry(), false);
        let package_data = test_package_data(Some(jwks_server.url));
        let dest_dir = tempdir().unwrap();
        let local_agent_package = downloader.download(&package_data, dest_dir.path()).unwrap();

        assert_eq!(
            std::fs::read(local_agent_package.path()).unwrap(),
            b"test agent package content"
        );
    }

    #[test]
    fn test_download_agent_package_success_signature_verification_enabled_but_no_public_key_informed()
     {
        let server = FakeOciServer::new(REPOSITORY, VERSION)
            .with_artifact_type(&ManifestArtifactType::AgentPackage.to_string())
            .with_layer(
                b"test agent package content",
                &LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerTarGz).to_string(),
            )
            .build();

        let downloader = create_downloader(server.registry(), true);
        let dest_dir = tempdir().unwrap();
        let package_data = test_package_data(None);
        let local_agent_package = downloader.download(&package_data, dest_dir.path()).unwrap();

        assert_eq!(
            std::fs::read(local_agent_package.path()).unwrap(),
            b"test agent package content"
        );
    }

    #[test]
    fn test_download_with_multiple_layers() {
        let server = FakeOciServer::new(REPOSITORY, VERSION)
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

        let package_data = test_package_data(None);
        let downloader = create_downloader(server.registry(), false);
        let dest_dir = tempdir().unwrap();
        let local_agent_package = downloader.download(&package_data, dest_dir.path()).unwrap();

        assert_eq!(
            std::fs::read(local_agent_package.path()).unwrap(),
            b"layer 1 content"
        );
    }

    #[test]
    fn test_download_with_invalid_package() {
        let server = FakeOciServer::new(REPOSITORY, VERSION)
            .with_layer(
                b"test content",
                &LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerTarGz).to_string(),
            )
            .with_artifact_type("application/vnd.unknown.type.v1")
            .build();

        let downloader = create_downloader(server.registry(), false);
        let dest_dir = tempdir().unwrap();
        let package_data = test_package_data(None);
        let err = downloader
            .download(&package_data, dest_dir.path())
            .unwrap_err();
        assert!(err.to_string().contains("validating package manifest"));
    }

    fn create_downloader(
        registry: String,
        signature_verification_enabled: bool,
    ) -> OCIPackageArtifactDownloader {
        let client = Client::try_new(
            ClientConfig {
                protocol: ClientProtocol::Http,
                ..Default::default()
            },
            ProxyConfig::default(),
            tokio_runtime(),
        )
        .unwrap();
        OCIPackageArtifactDownloader::new(
            client,
            Registry::from_str(&registry).unwrap(),
            None,
            signature_verification_enabled,
        )
    }
}
