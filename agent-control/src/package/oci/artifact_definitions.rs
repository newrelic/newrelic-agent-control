use std::fmt::Display;
use std::path::{Path, PathBuf};

use crate::utils::extract::{extract_tar_gz, extract_zip};
use oci_client::manifest::{OciDescriptor, OciImageManifest};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct DefinitionError(String);

const AGENT_PACKAGE_ARTIFACT_TYPE: &str = "application/vnd.newrelic.agent.v1";
const AGENT_TYPE_ARTIFACT_TYPE: &str = "application/vnd.newrelic.agent-type.v1";
/// OCI manifest artifact types supported.
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

#[derive(Debug)]
pub enum PackageMediaType {
    AgentPackageLayerTarGz,
    AgentPackageLayerZip,
}
impl Display for PackageMediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageMediaType::AgentPackageLayerTarGz => write!(f, "{}", AGENT_PACKAGE_LAYER_TAR_GZ),
            PackageMediaType::AgentPackageLayerZip => write!(f, "{}", AGENT_PACKAGE_LAYER_ZIP),
        }
    }
}

/// Represents a Agent Package OCI artifact locally stored.
#[derive(Debug)]
pub struct LocalAgentPackage {
    blob_path: PathBuf,
    blob_media_type: PackageMediaType,
}
impl LocalAgentPackage {
    pub fn new(blob_media_type: PackageMediaType, blob_path: PathBuf) -> Self {
        Self {
            blob_media_type,
            blob_path,
        }
    }

    /// Extracts the agent package to the specified destination path.
    pub fn extract(&self, dest_path: &Path) -> Result<(), DefinitionError> {
        match &self.blob_media_type {
            PackageMediaType::AgentPackageLayerTarGz => extract_tar_gz(&self.blob_path, dest_path),
            PackageMediaType::AgentPackageLayerZip => extract_zip(&self.blob_path, dest_path),
        }
        .map_err(|e| DefinitionError(format!("failed extracting: {e}")))
    }

    /// Validates that the manifest meets the requirements for an Agent Package artifact and
    /// returns the layer descriptor that contains the package blob with its media type.
    /// Agent Package Manifest requirements:
    /// - artifactType must be '[AGENT_PACKAGE_ARTIFACT_TYPE]'
    /// - exactly one layer with mediaType of '[PackageMediaType]'
    pub fn get_layer(
        manifest: &OciImageManifest,
    ) -> Result<(OciDescriptor, PackageMediaType), DefinitionError> {
        if manifest.artifact_type.as_deref() != Some(AGENT_PACKAGE_ARTIFACT_TYPE) {
            return Err(DefinitionError(format!(
                "invalid artifactType: expected {}, got {:?}",
                AGENT_PACKAGE_ARTIFACT_TYPE, manifest.artifact_type
            )));
        }
        let mut supported_layers = manifest.layers.iter().filter_map(|layer| {
            match LayerMediaType::from(layer.media_type.as_str()) {
                LayerMediaType::AgentPackage(pkg_media_type) => Some((layer, pkg_media_type)),
                _ => None,
            }
        });

        let Some((layer, media_type)) = supported_layers.next() else {
            return Err(DefinitionError(format!(
                "agent package artifact must have at least one supported layer {} or {}",
                PackageMediaType::AgentPackageLayerTarGz,
                PackageMediaType::AgentPackageLayerZip
            )));
        };
        if supported_layers.next().is_some() {
            return Err(DefinitionError(
                "agent package artifact must have exactly one supported layer".to_string(),
            ));
        }
        Ok((layer.clone(), media_type))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use assert_matches::assert_matches;

    #[rstest::rstest]
    #[case::tar_gz_single_layer(
        vec![AGENT_PACKAGE_LAYER_TAR_GZ]
    )]
    #[case::zip_single_layer(
        vec![AGENT_PACKAGE_LAYER_ZIP]
    )]
    #[case::tar_gz_with_extra_layers(
        vec![AGENT_PACKAGE_LAYER_TAR_GZ, "application/vnd.custom.extra.v1"]
    )]
    #[case::zip_with_extra_layers(
        vec![AGENT_PACKAGE_LAYER_ZIP, "application/vnd.custom.extra.v1"]
    )]
    fn test_local_artifact_to_agent_package_success(#[case] layer_media_types: Vec<&str>) {
        let layers = layer_media_types
            .iter()
            .map(|media_type| OciDescriptor {
                media_type: media_type.to_string(),
                ..Default::default()
            })
            .collect();
        let manifest = OciImageManifest {
            artifact_type: Some(ManifestArtifactType::AgentPackage.to_string()),
            layers,
            ..Default::default()
        };

        let (_, media_type) = LocalAgentPackage::get_layer(&manifest).unwrap();
        match layer_media_types[0] {
            AGENT_PACKAGE_LAYER_TAR_GZ => {
                assert_matches!(media_type, PackageMediaType::AgentPackageLayerTarGz)
            }
            AGENT_PACKAGE_LAYER_ZIP => {
                assert_matches!(media_type, PackageMediaType::AgentPackageLayerZip)
            }
            _ => panic!("unexpected media type"),
        }
    }
    #[rstest::rstest]
    #[case::invalid_artifact_type(
        "application/vnd.newrelic.unknown.v1",
        vec![],
        "invalid artifactType"
    )]
    #[case::no_supported_layers(
        AGENT_PACKAGE_ARTIFACT_TYPE,
        vec!["application/vnd.custom.extra.v1"],
        "must have at least one supported layer"
    )]
    #[case::empty_layers(
        AGENT_PACKAGE_ARTIFACT_TYPE,
        vec![],
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
        #[case] expected_error: &str,
    ) {
        let layers = layer_media_types
            .iter()
            .map(|media_type| OciDescriptor {
                media_type: media_type.to_string(),
                ..Default::default()
            })
            .collect();
        let manifest = OciImageManifest {
            artifact_type: Some(artifact_type.to_string()),
            layers,
            ..Default::default()
        };
        let err = LocalAgentPackage::get_layer(&manifest).unwrap_err();
        assert!(err.to_string().contains(expected_error));
    }
}
