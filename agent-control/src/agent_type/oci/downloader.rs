use std::time::Duration;

use oci_client::Reference;
use oci_client::secrets::RegistryAuth;
use tracing::{debug, warn};
use url::Url;

use crate::agent_control::config::{OciAuth, Registry};
use crate::agent_type::oci::AgentTypeTag;
use crate::agent_type::runtime_config::on_host::package::rendered::Repository;
use crate::oci::artifact_definitions::LocalAgentType;
use crate::oci::{Client, OciArtifactFetcher, OciClientError};

/// Maximum size of an agent type artifact blob (generous upper bound).
const MAX_ARTIFACT_SIZE_BYTES: usize = 10 * 1024 * 1024; // 10 MiB

/// An interface for downloading Agent Type definitions from a configured OCI remote.
pub trait OCIAgentTypeDownloader: Send + Sync {
    /// Error returned when a download fails. Consumers only rely on its `Display` representation,
    /// so each implementation (and its mock) can use its own error type.
    type Error: std::error::Error;

    /// Downloads and verifies the agent type artifact identified by `tag`, returning the raw bytes
    /// of the single agent type definition it contains.
    fn download(&self, tag: &AgentTypeTag) -> Result<Vec<u8>, Self::Error>;
}

/// Downloads agent type definitions from a configured OCI remote into memory.
///
/// It represents a single remote source: registry, repository and signing configuration are fixed;
/// only the artifact `tag` varies per download.
///
/// The artifact content is never written to disk, so resolution does not depend on a writable
/// filesystem location (relevant on Kubernetes). Blobs larger than `max_size_bytes` are rejected.
pub struct OCIAgentTypeArtifactDownloader {
    fetcher: OciArtifactFetcher,
    registry: Registry,
    repository: Repository,
    public_key_url: Option<Url>,
    max_size_bytes: usize,
}

impl OCIAgentTypeDownloader for OCIAgentTypeArtifactDownloader {
    type Error = OciClientError;

    /// Downloads the agent type artifact at `<registry>/<repository>:<tag>`.
    ///
    /// If signature verification is enabled and a `public_key_url` is configured, it first verifies
    /// the artifact's signature and then downloads the artifact that was verified (identified by
    /// `digest`, to assure the artifact downloaded is the one verified).
    ///
    /// On failure the operation is retried as configured; if all attempts are exhausted it returns
    /// an error.
    fn download(&self, tag: &AgentTypeTag) -> Result<Vec<u8>, OciClientError> {
        let base_reference = Reference::with_tag(
            self.registry.to_string(),
            self.repository.to_string(),
            tag.as_str().to_string(),
        );
        debug!(
            oci_reference = base_reference.to_string(),
            "Downloading Agent Type from remote repository"
        );
        let public_key_url = self.should_verify_signature();
        let max_size_bytes = self.max_size_bytes;
        self.fetcher.fetch(
            &base_reference,
            public_key_url,
            |client, reference, auth| {
                Self::download_definition(client, reference, auth, max_size_bytes)
            },
        )
    }
}

impl OCIAgentTypeArtifactDownloader {
    /// Returns a downloader with default (no) retries.
    pub fn new(
        client: Client,
        registry: Registry,
        repository: Repository,
        auth: Option<OciAuth>,
        public_key_url: Option<Url>,
    ) -> Self {
        Self {
            fetcher: OciArtifactFetcher::new(client, auth),
            registry,
            repository,
            public_key_url,
            max_size_bytes: MAX_ARTIFACT_SIZE_BYTES,
        }
    }

    /// Returns a new downloader with the provided retry configuration.
    pub fn with_retries(self, retries: usize, retry_interval: Duration) -> Self {
        Self {
            fetcher: self.fetcher.with_retries(retries, retry_interval),
            ..self
        }
    }

    /// Returns the configured `public_key_url` if signature verification needs to be performed,
    /// None otherwise.
    fn should_verify_signature(&self) -> Option<&Url> {
        if self.public_key_url.is_none() {
            warn!(
                repository = self.repository.to_string(),
                registry = self.registry.to_string(),
                "Signature verification is disabled, skipping"
            );
        }
        self.public_key_url.as_ref()
    }

    /// Pulls the manifest, validates it is an agent type artifact, pulls its single layer into
    /// memory and returns the agent type definition it contains.
    fn download_definition(
        client: &Client,
        reference: &Reference,
        auth: &RegistryAuth,
        max_size_bytes: usize,
    ) -> Result<Vec<u8>, OciClientError> {
        let (manifest, _) = client.pull_image_manifest(reference, auth).map_err(|err| {
            OciClientError::FetchArtifact(format!("downloading agent type manifest: {err}"))
        })?;

        let layer = LocalAgentType::get_layer(&manifest).map_err(|err| {
            OciClientError::FetchArtifact(format!("validating agent type manifest: {err}"))
        })?;

        let blob = client
            .pull_blob(reference, &layer, max_size_bytes)
            .map_err(|err| {
                OciClientError::FetchArtifact(format!("downloading agent type reference: {err}"))
            })?;

        LocalAgentType::new(blob)
            .extract_definition()
            .map_err(|err| {
                OciClientError::FetchArtifact(format!("extracting agent type definition: {err}"))
            })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::environment::Environment;
    use crate::http::config::ProxyConfig;
    use crate::oci::artifact_definitions::{LayerMediaType, ManifestArtifactType};
    use crate::oci::tests::FakeOciServer;
    use crate::utils::test_runtime::tokio_runtime;
    use assert_matches::assert_matches;
    use mockall::mock;
    use oci_client::client::{ClientConfig, ClientProtocol};
    use std::str::FromStr;

    /// A trivially-constructible error for the mock downloader, so consumers (e.g. the remote
    /// registry) can exercise the failure path without access to the real downloader's internals.
    #[derive(Debug, thiserror::Error)]
    #[error("{0}")]
    pub struct FakeDownloaderError(pub String);

    mock! {
        pub OCIAgentTypeDownloader {}
        impl OCIAgentTypeDownloader for OCIAgentTypeDownloader {
            type Error = FakeDownloaderError;
            fn download(&self, tag: &AgentTypeTag) -> Result<Vec<u8>, FakeDownloaderError>;
        }
    }

    impl OCIAgentTypeArtifactDownloader {
        /// Overrides the maximum artifact blob size to exercise the size cap.
        fn with_max_size_bytes(self, max_size_bytes: usize) -> Self {
            Self {
                max_size_bytes,
                ..self
            }
        }
    }

    const DEFINITION: &[u8] = b"namespace: some.namespace\nname: some.agent.type\n";
    const REPOSITORY: &str = "my-org/agent-types-repository";
    const TAG: &str = "host-linux-some.agent.type-0.0.42";

    /// An [AgentTypeTag] whose string equals [TAG], so it matches the artifact the mock server
    /// serves under that tag.
    fn agent_type_tag() -> AgentTypeTag {
        AgentTypeTag::new(
            &AgentTypeID::try_from("newrelic/some.agent.type:0.0.42").unwrap(),
            Environment::Linux,
        )
    }

    /// Builds an in-memory gzipped tar containing a single file with the given content.
    fn tar_gz_with_definition(file_name: &str, content: &[u8]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::GzEncoder;

        let enc = GzEncoder::new(Vec::new(), Compression::default());
        let mut tar = tar::Builder::new(enc);
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, file_name, content).unwrap();
        tar.into_inner().unwrap().finish().unwrap()
    }

    fn create_downloader(
        registry: String,
        public_key_url: Option<Url>,
    ) -> OCIAgentTypeArtifactDownloader {
        let client = Client::try_new(
            ClientConfig {
                protocol: ClientProtocol::Http,
                ..Default::default()
            },
            ProxyConfig::default(),
            tokio_runtime(),
        )
        .unwrap();
        OCIAgentTypeArtifactDownloader::new(
            client,
            Registry::from_str(&registry).unwrap(),
            Repository::from_str(REPOSITORY).unwrap(),
            None,
            public_key_url,
        )
    }

    #[test]
    fn test_download_success_signature_disabled() {
        let tar_gz = tar_gz_with_definition(&format!("{TAG}.yaml"), DEFINITION);
        let server = FakeOciServer::new(REPOSITORY, TAG)
            .with_artifact_type(&ManifestArtifactType::AgentType.to_string())
            .with_layer(&tar_gz, &LayerMediaType::AgentType.to_string())
            .build();

        let downloader = create_downloader(server.registry(), None);
        let definition = downloader.download(&agent_type_tag()).unwrap();
        assert_eq!(definition, DEFINITION);
    }

    #[test]
    fn test_download_invalid_artifact_type() {
        let tar_gz = tar_gz_with_definition(&format!("{TAG}.yaml"), DEFINITION);
        let server = FakeOciServer::new(REPOSITORY, TAG)
            .with_artifact_type("application/vnd.unknown.v1")
            .with_layer(&tar_gz, &LayerMediaType::AgentType.to_string())
            .build();

        let downloader = create_downloader(server.registry(), None);
        assert_matches!(downloader.download(&agent_type_tag()), Err(OciClientError::AttemptsExceeded(msg)) => {
            assert!(msg.contains("validating agent type manifest"), "{msg}");
        });
    }

    #[test]
    fn test_download_exceeds_max_size() {
        let tar_gz = tar_gz_with_definition(&format!("{TAG}.yaml"), DEFINITION);
        let server = FakeOciServer::new(REPOSITORY, TAG)
            .with_artifact_type(&ManifestArtifactType::AgentType.to_string())
            .with_layer(&tar_gz, &LayerMediaType::AgentType.to_string())
            .build();

        let downloader = create_downloader(server.registry(), None).with_max_size_bytes(10);
        assert_matches!(downloader.download(&agent_type_tag()), Err(OciClientError::AttemptsExceeded(msg)) => {
            assert!(msg.contains("exceeds maximum"), "{msg}");
        });
    }
}
