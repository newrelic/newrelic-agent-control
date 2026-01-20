use std::fmt::Display;
use std::path::{Path, PathBuf};

use crate::utils::extract::{extract_tar_gz, extract_zip};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct DefinitionError(String);

const AGENT_PACKAGE_ARTIFACT_TYPE: &str = "application/vnd.newrelic.agent.v1";
const AGENT_TYPE_ARTIFACT_TYPE: &str = "application/vnd.newrelic.agent-type.v1";
/// OCI manifestartifact types supported.
#[derive(Debug)]
pub enum ManifestArtifactType {
    AgentPackage,
    AgentType,
}
impl TryFrom<&str> for ManifestArtifactType {
    type Error = DefinitionError;

    fn try_from(artifact_type: &str) -> Result<Self, Self::Error> {
        match artifact_type {
            AGENT_PACKAGE_ARTIFACT_TYPE => Ok(ManifestArtifactType::AgentPackage),
            AGENT_TYPE_ARTIFACT_TYPE => Ok(ManifestArtifactType::AgentType),
            other => Err(DefinitionError(format!(
                "unsupported artifact type: {}",
                other
            ))),
        }
    }
}
impl Display for ManifestArtifactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestArtifactType::AgentPackage => write!(f, "{}", AGENT_PACKAGE_ARTIFACT_TYPE),
            ManifestArtifactType::AgentType => write!(f, "{}", AGENT_TYPE_ARTIFACT_TYPE),
        }
    }
}

const AGENT_PACKAGE_LAYER_TAR_GZ: &str = "application/vnd.newrelic.agent.content.v1.tar+gzip";
const AGENT_PACKAGE_LAYER_ZIP: &str = "application/vnd.newrelic.agent.content.v1.zip";
const AGENT_TYPE_LAYER_TAR_GZ: &str = "application/vnd.newrelic.agent-type.content.v1.tar+gzip";

/// OCI layer media types. Having the Other variant allows for future extensibility,
/// allowing us to fetch and use artifacts with unknown layers if needed.
#[derive(Debug)]
pub enum LayerMediaType {
    AgentPackage(PackageMediaType),
    AgentType,
    Other(String),
}
impl From<&str> for LayerMediaType {
    fn from(media_type: &str) -> Self {
        match media_type {
            AGENT_PACKAGE_LAYER_TAR_GZ => {
                LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerTarGz)
            }
            AGENT_PACKAGE_LAYER_ZIP => {
                LayerMediaType::AgentPackage(PackageMediaType::AgentPackageLayerZip)
            }
            AGENT_TYPE_LAYER_TAR_GZ => LayerMediaType::AgentType,
            other => LayerMediaType::Other(other.to_string()),
        }
    }
}
impl Display for LayerMediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayerMediaType::AgentPackage(pkg_media_type) => match pkg_media_type {
                PackageMediaType::AgentPackageLayerTarGz => {
                    write!(f, "{}", AGENT_PACKAGE_LAYER_TAR_GZ)
                }
                PackageMediaType::AgentPackageLayerZip => {
                    write!(f, "{}", AGENT_PACKAGE_LAYER_ZIP)
                }
            },
            LayerMediaType::AgentType => write!(f, "{}", AGENT_TYPE_LAYER_TAR_GZ),
            LayerMediaType::Other(other) => write!(f, "{}", other),
        }
    }
}
/// Represents a OCI artifact locally stored with its complete metadata.
#[derive(Debug)]
pub struct LocalArtifact {
    artifact_type: ManifestArtifactType,
    blobs: Vec<LocalBlob>,
}
impl LocalArtifact {
    pub fn new(artifact_type: ManifestArtifactType, blobs: Vec<LocalBlob>) -> Self {
        Self {
            artifact_type,
            blobs,
        }
    }
}

#[derive(Debug)]
pub enum PackageMediaType {
    AgentPackageLayerTarGz,
    AgentPackageLayerZip,
}
/// Represents a OCI blob locally stored.
#[derive(Debug)]
pub struct LocalBlob {
    media_type: LayerMediaType,
    path: PathBuf,
}

impl LocalBlob {
    pub fn new(media_type: LayerMediaType, path: PathBuf) -> Self {
        Self { media_type, path }
    }
}

/// Represents a Agent Package OCI artifact locally stored.
/// Agent Package Manifest requirements:
/// - artifactType must be '[AGENT_PACKAGE_ARTIFACT_TYPE]'
/// - exactly one layer with mediaType of '[PackageMediaType]'
#[derive(Debug)]
pub struct LocalAgentPackage {
    blob_path: PathBuf,
    blob_media_type: PackageMediaType,
}
impl LocalAgentPackage {
    /// Extracts the agent package to the specified destination path.
    pub fn extract(&self, dest_path: &Path) -> Result<(), DefinitionError> {
        match &self.blob_media_type {
            PackageMediaType::AgentPackageLayerTarGz => extract_tar_gz(&self.blob_path, dest_path),
            PackageMediaType::AgentPackageLayerZip => extract_zip(&self.blob_path, dest_path),
        }
        .map_err(|e| DefinitionError(format!("failed extracting: {e}")))
    }
}

impl TryFrom<LocalArtifact> for LocalAgentPackage {
    type Error = DefinitionError;
    fn try_from(value: LocalArtifact) -> Result<Self, Self::Error> {
        match value.artifact_type {
            ManifestArtifactType::AgentPackage => {}
            _ => {
                return Err(DefinitionError(format!(
                    "invalid artifact type: expected {}, got {}",
                    ManifestArtifactType::AgentPackage,
                    value.artifact_type
                )));
            }
        }
        let mut blobs = value
            .blobs
            .into_iter()
            .filter_map(|blob| match blob.media_type {
                LayerMediaType::AgentPackage(pkg_media_type) => Some((blob.path, pkg_media_type)),
                _ => None,
            });
        let Some(blob) = blobs.next() else {
            return Err(DefinitionError(
                "agent package artifact must have at least one supported layer".to_string(),
            ));
        };
        if blobs.next().is_some() {
            return Err(DefinitionError(
                "agent package artifact must have exactly one supported layer".to_string(),
            ));
        }
        Ok(Self {
            blob_path: blob.0,
            blob_media_type: blob.1,
        })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    impl LocalAgentPackage {
        pub fn new(file_path: PathBuf) -> Self {
            Self {
                blob_path: file_path,
                blob_media_type: PackageMediaType::AgentPackageLayerTarGz,
            }
        }
    }

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
    fn test_local_artifact_to_agent_package_success(
        #[case] artifact_type: &str,
        #[case] layer_media_types: Vec<&str>,
    ) {
        let blobs: Vec<LocalBlob> = layer_media_types
            .iter()
            .map(|media_type| create_blob(media_type))
            .collect();

        let artifact_type = ManifestArtifactType::try_from(artifact_type).unwrap();
        let artifact = LocalArtifact::new(artifact_type, blobs);

        LocalAgentPackage::try_from(artifact).unwrap();
    }

    #[rstest::rstest]
    #[case::no_layers(
        AGENT_PACKAGE_ARTIFACT_TYPE,
        vec![],
        "must have at least one supported layer"
    )]
    #[case::only_unsupported_layers(
        AGENT_PACKAGE_ARTIFACT_TYPE,
        vec!["application/vnd.unsupported.v1", "application/octet-stream"],
        "must have at least one supported layer"
    )]
    #[case::multiple_supported_layers(
        AGENT_PACKAGE_ARTIFACT_TYPE,
        vec![AGENT_PACKAGE_LAYER_TAR_GZ, AGENT_PACKAGE_LAYER_ZIP],
        "must have exactly one supported layer"
    )]
    fn test_local_artifact_to_agent_package_failure(
        #[case] artifact_type: &str,
        #[case] layer_media_types: Vec<&str>,
        #[case] expected_error_fragment: &str,
    ) {
        let blobs: Vec<LocalBlob> = layer_media_types
            .iter()
            .map(|media_type| create_blob(media_type))
            .collect();

        let artifact_type = ManifestArtifactType::try_from(artifact_type).unwrap();
        let artifact = LocalArtifact::new(artifact_type, blobs);

        let err = LocalAgentPackage::try_from(artifact).unwrap_err();
        assert!(err.to_string().contains(expected_error_fragment));
    }

    fn create_blob(media_type: &str) -> LocalBlob {
        LocalBlob::new(
            LayerMediaType::from(media_type),
            PathBuf::from("/dummy/path.tar.gz"),
        )
    }
}
