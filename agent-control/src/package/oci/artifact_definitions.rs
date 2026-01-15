use std::fmt::Display;
use std::path::{Path, PathBuf};

use oci_client::manifest::{OciDescriptor, OciImageManifest};

use crate::utils::extract::{extract_tar_gz, extract_zip};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct DefinitionError(String);

/// OCI artifact types
pub enum ArtifactType {
    AgentPackage,
    AgentType,
    Other(String),
}
const AGENT_PACKAGE_ARTIFACT_TYPE: &str = "application/vnd.newrelic.agent.v1";
const AGENT_TYPE_ARTIFACT_TYPE: &str = "application/vnd.newrelic.agent-type.v1+json";
impl From<&str> for ArtifactType {
    fn from(artifact_type: &str) -> Self {
        match artifact_type {
            AGENT_PACKAGE_ARTIFACT_TYPE => ArtifactType::AgentPackage,
            AGENT_TYPE_ARTIFACT_TYPE => ArtifactType::AgentType,
            other => ArtifactType::Other(other.to_string()),
        }
    }
}
impl Display for ArtifactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactType::AgentPackage => write!(f, "{}", AGENT_PACKAGE_ARTIFACT_TYPE),
            ArtifactType::AgentType => write!(f, "{}", AGENT_TYPE_ARTIFACT_TYPE),
            ArtifactType::Other(s) => write!(f, "{}", s),
        }
    }
}

/// OCI media types
pub enum MediaType {
    AgentPackageLayerTarGz,
    AgentPackageLayerZip,
    AgentTypeLayerTarGz,
    Other(String),
}
const AGENT_PACKAGE_LAYER_TAR_GZ: &str = "application/vnd.newrelic.agent.content.v1.tar+gzip";
const AGENT_PACKAGE_LAYER_ZIP: &str = "application/vnd.newrelic.agent.content.v1.zip";
const AGENT_TYPE_LAYER_TAR_GZ: &str = "application/vnd.newrelic.agent-type.content.v1.tar+gzip";

impl Display for MediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaType::AgentPackageLayerTarGz => write!(f, "{}", AGENT_PACKAGE_LAYER_TAR_GZ),
            MediaType::AgentPackageLayerZip => write!(f, "{}", AGENT_PACKAGE_LAYER_ZIP),
            MediaType::AgentTypeLayerTarGz => write!(f, "{}", AGENT_TYPE_LAYER_TAR_GZ),
            MediaType::Other(s) => write!(f, "{}", s),
        }
    }
}
impl From<&str> for MediaType {
    fn from(media_type: &str) -> Self {
        match media_type {
            AGENT_PACKAGE_LAYER_TAR_GZ => MediaType::AgentPackageLayerTarGz,
            AGENT_PACKAGE_LAYER_ZIP => MediaType::AgentPackageLayerZip,
            AGENT_TYPE_LAYER_TAR_GZ => MediaType::AgentTypeLayerTarGz,
            other => MediaType::Other(other.to_string()),
        }
    }
}

/// Represents a OCI artifact locally stored with its complete metadata.
#[derive(Debug)]
pub struct LocalArtifact {
    manifest: OciImageManifest,
    blobs: Vec<LocalBlob>,
}
impl LocalArtifact {
    pub fn new(manifest: OciImageManifest, blobs: Vec<LocalBlob>) -> Self {
        Self { manifest, blobs }
    }
}

/// Represents a OCI blob locally stored.
#[derive(Debug)]
pub struct LocalBlob {
    descriptor: OciDescriptor,
    path: PathBuf,
}

impl LocalBlob {
    pub fn new(descriptor: OciDescriptor, path: PathBuf) -> Self {
        Self { descriptor, path }
    }
    fn media_type(&self) -> MediaType {
        MediaType::from(self.descriptor.media_type.as_str())
    }
}

/// Represents a Agent Package OCI artifact locally stored.
/// Agent Package Manifest requirements:
/// - artifactType must be '[AGENT_PACKAGE_ARTIFACT_TYPE]'
/// - at least one layer with mediaType of either
///   '[AGENT_PACKAGE_LAYER_TAR_GZ]' or '[AGENT_PACKAGE_LAYER_ZIP]'
#[derive(Debug)]
pub struct LocalAgentPackage(LocalArtifact);
impl LocalAgentPackage {
    /// Extracts the agent package to the specified destination path.
    pub fn extract(&self, dest_path: &Path) -> Result<(), DefinitionError> {
        let blob = self.package_blob()?;

        match blob.media_type() {
            MediaType::AgentPackageLayerTarGz => extract_tar_gz(&blob.path, dest_path),
            MediaType::AgentPackageLayerZip => extract_zip(&blob.path, dest_path),
            other => Err(DefinitionError(format!(
                "unsupported media type '{other}' for agent package"
            )))?,
        }
        .map_err(|e| DefinitionError(format!("failed extracting: {e}")))
    }
    /// Retrieves the blob that contains the agent package data.
    fn package_blob(&self) -> Result<&LocalBlob, DefinitionError> {
        let Some(blob) = self.0.blobs.iter().find(|blob| {
            matches!(
                blob.media_type(),
                MediaType::AgentPackageLayerTarGz | MediaType::AgentPackageLayerZip
            )
        }) else {
            return Err(DefinitionError("agent package layer missing".to_string()));
        };
        Ok(blob)
    }

    /// Validates that the provided OCI artifact is a valid agent package.
    fn valid_agent_package(artifact: &LocalArtifact) -> Result<(), DefinitionError> {
        let Some(artifact_type) = &artifact.manifest.artifact_type else {
            return Err(DefinitionError("missing artifact type".to_string()));
        };
        match ArtifactType::from(artifact_type.as_str()) {
            ArtifactType::AgentPackage => {}
            _ => {
                return Err(DefinitionError(format!(
                    "invalid artifact type: expected {}, got {}",
                    ArtifactType::AgentPackage,
                    artifact_type
                )));
            }
        }

        // Validate that at least one supported layer is present. Allowing extra layers for future use cases.
        if !artifact.blobs.iter().any(|blob| {
            matches!(
                blob.media_type(),
                MediaType::AgentPackageLayerTarGz | MediaType::AgentPackageLayerZip
            )
        }) {
            return Err(DefinitionError(
                "agent package artifact must have at least one supported layer".to_string(),
            ));
        }
        Ok(())
    }
}

impl TryFrom<LocalArtifact> for LocalAgentPackage {
    type Error = DefinitionError;

    fn try_from(artifact: LocalArtifact) -> Result<Self, Self::Error> {
        Self::valid_agent_package(&artifact)?;
        Ok(LocalAgentPackage(artifact))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    impl LocalAgentPackage {
        pub fn new(file_path: PathBuf) -> Self {
            let descriptor = OciDescriptor {
                media_type: AGENT_PACKAGE_LAYER_TAR_GZ.to_string(),
                ..Default::default()
            };
            let manifest = OciImageManifest {
                artifact_type: Some(ArtifactType::AgentPackage.to_string()),
                layers: vec![descriptor.clone()],
                ..Default::default()
            };
            let blob = LocalBlob::new(descriptor, file_path);
            let artifact = LocalArtifact::new(manifest, vec![blob]);
            Self(artifact)
        }
    }
    impl Default for LocalAgentPackage {
        fn default() -> Self {
            let artifact = LocalArtifact::new(OciImageManifest::default(), vec![]);
            Self(artifact)
        }
    }

    // ========== Test Fixtures ==========

    fn dummy_path() -> PathBuf {
        PathBuf::from("/dummy/path.tar.gz")
    }

    fn create_blob(media_type: &str, path: PathBuf) -> LocalBlob {
        LocalBlob::new(
            OciDescriptor {
                media_type: media_type.to_string(),
                digest: "sha256:1234567890abcdef".to_string(),
                size: 1024,
                ..Default::default()
            },
            path,
        )
    }

    fn create_manifest(
        artifact_type: Option<String>,
        layers: Vec<OciDescriptor>,
    ) -> OciImageManifest {
        OciImageManifest {
            artifact_type,
            layers,
            ..Default::default()
        }
    }

    // ========== Valid Conversion Tests ==========

    #[rstest::rstest]
    #[case::tar_gz_single_layer(
        AGENT_PACKAGE_ARTIFACT_TYPE,
        vec![AGENT_PACKAGE_LAYER_TAR_GZ]
    )]
    #[case::zip_single_layer(
        AGENT_PACKAGE_ARTIFACT_TYPE,
        vec![AGENT_PACKAGE_LAYER_ZIP]
    )]
    #[case::tar_gz_with_extra_layers(
        AGENT_PACKAGE_ARTIFACT_TYPE,
        vec![AGENT_PACKAGE_LAYER_TAR_GZ, "application/vnd.custom.extra.v1"]
    )]
    #[case::zip_with_extra_layers(
        AGENT_PACKAGE_ARTIFACT_TYPE,
        vec![AGENT_PACKAGE_LAYER_ZIP, "application/vnd.custom.extra.v1"]
    )]
    #[case::multiple_supported_layers(
        AGENT_PACKAGE_ARTIFACT_TYPE,
        vec![AGENT_PACKAGE_LAYER_TAR_GZ, AGENT_PACKAGE_LAYER_ZIP]
    )]
    fn test_local_artifact_to_agent_package_success(
        #[case] artifact_type: &str,
        #[case] layer_media_types: Vec<&str>,
    ) {
        let blobs: Vec<LocalBlob> = layer_media_types
            .iter()
            .map(|media_type| create_blob(media_type, dummy_path()))
            .collect();

        let layers: Vec<OciDescriptor> = blobs.iter().map(|b| b.descriptor.clone()).collect();

        let manifest = create_manifest(Some(artifact_type.to_string()), layers);
        let artifact = LocalArtifact::new(manifest, blobs);

        LocalAgentPackage::try_from(artifact).unwrap();
    }

    // ========== Failure Conversion Tests ==========

    #[rstest::rstest]
    #[case::missing_artifact_type(
        None,
        vec![AGENT_PACKAGE_LAYER_TAR_GZ],
        "missing artifact type"
    )]
    #[case::wrong_artifact_type(
        Some(AGENT_TYPE_ARTIFACT_TYPE),
        vec![AGENT_PACKAGE_LAYER_TAR_GZ],
        "invalid artifact type"
    )]
    #[case::custom_artifact_type(
        Some("application/vnd.custom.artifact.v1"),
        vec![AGENT_PACKAGE_LAYER_TAR_GZ],
        "invalid artifact type"
    )]
    #[case::no_layers(
        Some(AGENT_PACKAGE_ARTIFACT_TYPE),
        vec![],
        "must have at least one supported layer"
    )]
    #[case::only_unsupported_layers(
        Some(AGENT_PACKAGE_ARTIFACT_TYPE),
        vec!["application/vnd.unsupported.v1", "application/octet-stream"],
        "must have at least one supported layer"
    )]
    #[case::agent_type_layer_instead(
        Some(AGENT_PACKAGE_ARTIFACT_TYPE),
        vec![AGENT_TYPE_LAYER_TAR_GZ],
        "must have at least one supported layer"
    )]
    fn test_local_artifact_to_agent_package_failure(
        #[case] artifact_type: Option<&str>,
        #[case] layer_media_types: Vec<&str>,
        #[case] expected_error_fragment: &str,
    ) {
        let blobs: Vec<LocalBlob> = layer_media_types
            .iter()
            .map(|media_type| create_blob(media_type, dummy_path()))
            .collect();

        let layers: Vec<OciDescriptor> = blobs.iter().map(|b| b.descriptor.clone()).collect();

        let manifest = create_manifest(artifact_type.map(String::from), layers);
        let artifact = LocalArtifact::new(manifest, blobs);

        let err = LocalAgentPackage::try_from(artifact).unwrap_err();
        assert!(err.to_string().contains(expected_error_fragment));
    }
}
