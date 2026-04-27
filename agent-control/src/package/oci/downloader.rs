use crate::agent_control::config::OciAuth;
use crate::agent_control::config::Registry;
use crate::oci::{Client, reference_parser::ReferenceParser};
use crate::package::manager::PackageData;
use crate::package::oci::artifact_definitions::LocalAgentPackage;
use crate::utils::retry::retry;
use oci_client::Reference;
use oci_client::secrets::RegistryAuth;
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, warn};
use url::Url;

#[derive(Debug, Error)]
#[error("downloading OCI artifact: {0}")]
pub struct OCIDownloaderError(pub(super) String);

/// An interface for downloading Agent Packages from an OCI registry.
pub trait OCIAgentDownloader: Send + Sync {
    fn download(
        &self,
        package_data: &PackageData,
        destination_dir: &Path,
    ) -> Result<LocalAgentPackage, OCIDownloaderError>;
}

// This is expected to be thread-safe since it is used in the package manager.
// Make sure that we are not writing to disk to the same location from multiple threads.
// This implementation avoids that since each download is expected to be done in a separate package directory.
pub struct OCIArtifactDownloader {
    client: Client,
    auth: RegistryAuth,
    signature_verification_enabled: bool,
    max_retries: usize,
    retry_interval: Duration,
    registry: Registry,
}

impl OCIAgentDownloader for OCIArtifactDownloader {
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
    ) -> Result<LocalAgentPackage, OCIDownloaderError> {
        debug!(
            "Downloading from repository '{}' with version '{}'",
            package_data.repository, package_data.version
        );
        retry(self.max_retries, self.retry_interval, || {
            // Verify signature when needed
            let base_reference = ReferenceParser::from((
                &self.registry,
                &package_data.repository,
                &package_data.version,
            ))
            .into();

            let reference =
                if let Some(pk_url) = self.should_verify_signature(&package_data.public_key_url) {
                    &self.verified_package_signature_reference(&base_reference, pk_url)?
                } else {
                    &base_reference
                };
            // Download the package
            self.download_package_artifact(reference, package_dir)
                .inspect_err(|e| debug!("Download '{reference}' failed with error: {e}"))
        })
        .map_err(|e| OCIDownloaderError(format!("download attempts exceeded. Last error: {e}")))
    }
}

const DEFAULT_RETRIES: usize = 0;

impl OCIArtifactDownloader {
    /// Returns an artifact downloader with default retries setup.
    pub fn new(
        client: Client,
        registry: Registry,
        auth: Option<OciAuth>,
        signature_verification_enabled: bool,
    ) -> Self {
        OCIArtifactDownloader {
            client,
            auth: auth
                .as_ref()
                .map(RegistryAuth::from)
                .unwrap_or(RegistryAuth::Anonymous),
            signature_verification_enabled,
            max_retries: DEFAULT_RETRIES,
            retry_interval: Duration::default(),
            registry,
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

    /// Returns the [Reference] after verifying its signature. The reference always includes the `digest` to
    /// assure it is the same reference whose signature was verified.
    /// It returns an error if signature verification fails.
    fn verified_package_signature_reference(
        &self,
        reference: &Reference,
        public_key_url: &Url,
    ) -> Result<Reference, OCIDownloaderError> {
        self.client
            .verify_signature(reference, public_key_url, &self.auth)
            .map_err(|err| OCIDownloaderError(err.to_string()))
    }

    fn download_package_artifact(
        &self,
        reference: &Reference,
        package_dir: &Path,
    ) -> Result<LocalAgentPackage, OCIDownloaderError> {
        let (image_manifest, _) = self
            .client
            .pull_image_manifest(reference, &self.auth)
            .map_err(|err| OCIDownloaderError(format!("pull artifact manifest failure: {err}")))?;

        let (layer, media_type) = LocalAgentPackage::get_layer(&image_manifest)
            .map_err(|err| OCIDownloaderError(format!("validating package manifest: {err}")))?;

        let layer_path = package_dir.join(layer.digest.replace(':', "_"));
        self.client
            .pull_blob_to_file(reference, &layer, &layer_path)
            .map_err(|err| OCIDownloaderError(format!("download artifact failure: {err}")))?;

        debug!("Artifact written to {}", layer_path.display());

        Ok(LocalAgentPackage::new(media_type, layer_path))
    }
}

#[cfg(test)]
pub mod tests {
    use std::str::FromStr;

    use crate::agent_type::runtime_config::on_host::package::rendered::{Repository, Version};
    use crate::http::config::ProxyConfig;

    use crate::oci::tests::FakeOciServer;
    use crate::package::oci::artifact_definitions::{
        LayerMediaType, ManifestArtifactType, PackageMediaType,
    };
    use crate::signature::public_key::tests::TestKeyPair;
    use crate::signature::public_key_fetcher::tests::JwksMockServer;
    use crate::utils::test_runtime::tokio_runtime;

    use super::*;
    use assert_matches::assert_matches;
    use httpmock::prelude::*;
    use mockall::mock;

    use oci_client::client::{ClientConfig, ClientProtocol};
    use serde_json::json;

    use tempfile::tempdir;

    mock! {
        pub OCIDownloader {}
        impl OCIAgentDownloader for OCIDownloader {
            fn download(
                &self,
                package_data: &PackageData,
                package_dir: &Path,
            ) -> Result<LocalAgentPackage, OCIDownloaderError>;
        }
    }

    const REPOSITORY: &str = "test-repo";
    const VERSION: &str = "v1.0.0";

    fn test_package_data(public_key_url: Option<Url>) -> PackageData {
        PackageData {
            id: "test-package".to_string(),
            repository: Repository::from_str(REPOSITORY).unwrap(),
            version: Version::from_str(VERSION).unwrap(),
            public_key_url,
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

    #[test]
    fn test_download_with_missing_manifest() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/v2/test-repo/manifests/v1.0.0");
            then.status(404).json_body(json!({
                "errors": [{"code": "MANIFEST_UNKNOWN", "message": "manifest unknown"}]
            }));
        });

        let downloader = create_downloader(server.address().to_string(), false);
        let dest_dir = tempdir().unwrap();
        let package_data = test_package_data(None);
        let err = downloader
            .download(&package_data, dest_dir.path())
            .unwrap_err();
        assert!(
            err.to_string().contains("download attempts exceeded"),
            "{err}"
        );
    }

    #[test]
    fn test_download_with_unsigned_package() {
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
            .build(); // No signature

        let downloader = create_downloader(server.registry(), true);
        let dest_dir = tempdir().unwrap();
        let package_data = test_package_data(Some(jwks_server.url.clone()));
        let err = downloader
            .download(&package_data, dest_dir.path())
            .unwrap_err();
        assert!(err.to_string().contains("signature verification"), "{err}");
    }

    #[test]
    fn test_download_toctou_attack() {
        const ORIGINAL_CONTENT: &[u8] = b"A";
        const MALICIOUS_CONTENT: &[u8] = b"B";

        // Setup mock server with tag v1.0.0
        let key_pair = TestKeyPair::new(0);
        let jwks_server = JwksMockServer::new(vec![
            serde_json::to_value(key_pair.public_key_jwk()).unwrap(),
        ]);
        let oci_mock_a = FakeOciServer::new(REPOSITORY, VERSION)
            .with_artifact_type(&ManifestArtifactType::AgentPackage.to_string())
            .with_layer(
                ORIGINAL_CONTENT,
                &LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerTarGz).to_string(),
            )
            .with_signature(&key_pair);
        let server = MockServer::start();
        oci_mock_a.setup_mocks_on(&server);

        // Verify signature
        let reference = oci_mock_a.reference_on_server(&server);
        let downloader = create_downloader(server.address().to_string(), true);
        let verified_reference = downloader
            .verified_package_signature_reference(&reference, &jwks_server.url)
            .expect("Signature should be verified successfully");

        // Move tag v1.0.0 after signature is verified (TOCTOU attack)
        let oci_mock_b = FakeOciServer::new(REPOSITORY, VERSION)
            .with_artifact_type(&ManifestArtifactType::AgentPackage.to_string())
            .with_layer(
                MALICIOUS_CONTENT,
                &LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerTarGz).to_string(),
            );
        server.reset();
        oci_mock_b.setup_mocks_on(&server); // Setup the new tag first (takes precedence)
        oci_mock_a.setup_mocks_on(&server); // Also setup the previous (we need the previous digest and blobs)
        // Sanity check to assure that the tag was effectively moved
        let malicious_dest = tempdir().unwrap();
        let malicious_pkg = downloader
            .download_package_artifact(&reference, malicious_dest.path())
            .unwrap();
        assert_eq!(
            std::fs::read(malicious_pkg.path()).unwrap(),
            MALICIOUS_CONTENT
        );

        // The verified reference should still point to the original content
        let dest_dir = tempdir().unwrap();
        let local_agent_package = downloader
            .download_package_artifact(&verified_reference, dest_dir.path())
            .expect("Download should succeed");

        assert_eq!(
            std::fs::read(local_agent_package.path()).unwrap(),
            ORIGINAL_CONTENT
        );
    }

    #[test]
    fn test_download_man_in_the_middle_attack() {
        let oci_mock = FakeOciServer::new(REPOSITORY, VERSION)
            .with_artifact_type(&ManifestArtifactType::AgentPackage.to_string())
            .with_layer(
                b"some content",
                &LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerTarGz).to_string(),
            );
        let server = MockServer::start();
        // Content doesn't match the digest
        oci_mock.mock_manifest(
            &server,
            &oci_mock.manifest_digest(),
            b"malicious content".to_vec(),
        );
        let package_data = PackageData {
            id: "test-package".to_string(),
            repository: Repository::from_str("test-repo").unwrap(),
            version: Version::from_str(&format!("v1.0.0@{}", oci_mock.manifest_digest())).unwrap(),
            public_key_url: None,
        };

        let downloader = create_downloader(server.address().to_string(), false);
        let dest_dir = tempdir().unwrap();
        let result = downloader.download(&package_data, dest_dir.path());
        assert_matches!(result, Err(OCIDownloaderError(msg)) => {
            assert!(msg.contains("Digest error"));
        });
    }

    #[test]
    fn test_none_auth_defaults_to_anonymous() {
        let server = FakeOciServer::new(REPOSITORY, VERSION)
            .with_artifact_type(&ManifestArtifactType::AgentPackage.to_string())
            .with_layer(
                b"content",
                &LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerTarGz).to_string(),
            )
            .build();
        let package_data = test_package_data(None);

        let client = Client::try_new(
            ClientConfig {
                protocol: ClientProtocol::Http,
                ..Default::default()
            },
            ProxyConfig::default(),
            tokio_runtime(),
        )
        .unwrap();
        let downloader = OCIArtifactDownloader::new(
            client,
            Registry::from_str(&server.registry()).unwrap(),
            None,
            false,
        );
        let dest_dir = tempdir().unwrap();
        assert!(downloader.download(&package_data, dest_dir.path()).is_ok());
    }

    fn create_downloader(
        registry: String,
        signature_verification_enabled: bool,
    ) -> OCIArtifactDownloader {
        let client = Client::try_new(
            ClientConfig {
                protocol: ClientProtocol::Http,
                ..Default::default()
            },
            ProxyConfig::default(),
            tokio_runtime(),
        )
        .unwrap();
        OCIArtifactDownloader::new(
            client,
            Registry::from_str(&registry).unwrap(),
            None,
            signature_verification_enabled,
        )
    }
}
