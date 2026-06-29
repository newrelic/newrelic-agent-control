//! Definitions of the OCI artifact types Agent Control consumes (agent packages and agent
//! types), including their manifest/layer media types and helpers to validate manifests and
//! extract their content.

use std::fmt::Display;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::utils::extract::{extract_tar_gz, extract_zip};
use flate2::read::GzDecoder;
use oci_client::manifest::{OciDescriptor, OciImageManifest};
use tar::Archive;

/// Error returned when an OCI manifest does not meet the requirements of a known artifact type.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct DefinitionError(String);

/// OCI manifest artifact types supported.
#[derive(Debug)]
pub enum ManifestArtifactType {
    /// An agent package artifact (binaries/content to install an agent).
    AgentPackage,
    /// An agent type artifact (an Agent Type definition).
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

const AGENT_PACKAGE_ARTIFACT_TYPE: &str = "application/vnd.newrelic.agent.v1";
const AGENT_TYPE_ARTIFACT_TYPE: &str = "application/vnd.newrelic.agent-type.v1";

const AGENT_PACKAGE_LAYER_TAR_GZ: &str = "application/vnd.newrelic.agent.content.v1.tar+gzip";
const AGENT_PACKAGE_LAYER_ZIP: &str = "application/vnd.newrelic.agent.content.v1.zip";
const AGENT_TYPE_LAYER_TAR_GZ: &str = "application/vnd.newrelic.agent-type.content.v1.tar+gzip";

/// OCI layer media types. Having the Other variant allows for future extensibility,
/// allowing us to fetch and use artifacts with unknown layers if needed.
#[derive(Debug)]
pub enum LayerMediaType {
    /// An agent package content layer, with its specific package media type.
    AgentPackage(PackageMediaType),
    /// An agent type content layer.
    AgentType,
    /// Any other (unrecognized) layer media type, preserved verbatim.
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

/// Media type of an agent package content layer.
#[derive(Debug)]
pub enum PackageMediaType {
    /// A gzipped tar archive (`...content.v1.tar+gzip`).
    AgentPackageLayerTarGz,
    /// A zip archive (`...content.v1.zip`).
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
    /// Creates a handle to a locally stored agent package blob of the given media type.
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
    /// - artifactType must be '`AGENT_PACKAGE_ARTIFACT_TYPE`'
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

/// Represents an Agent Type OCI artifact held in memory.
///
/// An Agent Type artifact is a gzipped tar containing a single Agent Type definition YAML file.
/// It is kept in memory (instead of written to disk) so resolution does not depend on a writable
/// filesystem location, which is specially relevant on Kubernetes (read-only root fs, ephemeral
/// storage).
#[derive(Debug)]
pub struct LocalAgentType {
    blob: Vec<u8>,
}
impl LocalAgentType {
    /// Creates an in-memory handle to an agent type artifact blob.
    pub fn new(blob: Vec<u8>) -> Self {
        Self { blob }
    }

    /// Validates that the manifest meets the requirements for an Agent Type artifact and
    /// returns the layer descriptor that contains the definition blob.
    /// Agent Type Manifest requirements:
    /// - artifactType must be '`AGENT_TYPE_ARTIFACT_TYPE`'
    /// - exactly one agent-type layer (mediaType '`AGENT_TYPE_LAYER_TAR_GZ`'); other layers are ignored
    pub fn get_layer(manifest: &OciImageManifest) -> Result<OciDescriptor, DefinitionError> {
        if manifest.artifact_type.as_deref() != Some(AGENT_TYPE_ARTIFACT_TYPE) {
            return Err(DefinitionError(format!(
                "only '{}' artifact type is supported, got '{}'",
                AGENT_TYPE_ARTIFACT_TYPE,
                manifest.artifact_type.as_deref().unwrap_or_default()
            )));
        }
        let mut supported_layers = manifest.layers.iter().filter(|layer| {
            matches!(
                LayerMediaType::from(layer.media_type.as_str()),
                LayerMediaType::AgentType
            )
        });

        let Some(layer) = supported_layers.next() else {
            return Err(DefinitionError(format!(
                "agent type artifact must have one supported layer {}",
                LayerMediaType::AgentType
            )));
        };
        if supported_layers.next().is_some() {
            return Err(DefinitionError(
                "agent type artifact must have exactly one supported layer".to_string(),
            ));
        }
        Ok(layer.clone())
    }

    /// Decompresses the gzipped tar held in memory and returns the content of the single Agent
    /// Type definition file it contains.
    ///
    /// It fails if the artifact does not contain exactly one file. Deserializing the returned bytes
    /// into an agent type definition is left to the caller.
    pub fn extract_definition(self) -> Result<Vec<u8>, DefinitionError> {
        let mut archive = Archive::new(GzDecoder::new(self.blob.as_slice()));
        let entries = archive
            .entries()
            .map_err(|e| DefinitionError(format!("reading agent type artifact: {e}")))?;

        let mut definition: Option<Vec<u8>> = None;
        for entry in entries {
            let mut entry = entry
                .map_err(|e| DefinitionError(format!("reading agent type artifact entry: {e}")))?;
            // Skip non-file entries (e.g. directories).
            if !entry.header().entry_type().is_file() {
                continue;
            }
            if definition.is_some() {
                return Err(DefinitionError(
                    "agent type artifact must contain exactly one file".to_string(),
                ));
            }
            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .map_err(|e| DefinitionError(format!("reading agent type definition: {e}")))?;
            definition = Some(content);
        }

        definition.ok_or_else(|| {
            DefinitionError("agent type artifact does not contain any file".to_string())
        })
    }
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    use super::*;
    use assert_matches::assert_matches;

    impl LocalAgentPackage {
        pub fn path(&self) -> &PathBuf {
            &self.blob_path
        }
    }

    /// Builds an in-memory gzipped tar archive containing the provided `(name, content)` files.
    fn tar_gz_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::GzEncoder;

        let enc = GzEncoder::new(Vec::new(), Compression::default());
        let mut tar = tar::Builder::new(enc);
        for (name, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, name, *content).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }

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

    #[rstest::rstest]
    #[case::single_supported_layer(vec![AGENT_TYPE_LAYER_TAR_GZ])]
    #[case::ignores_extra_unsupported_layers(
        vec!["application/vnd.custom.extra.v1", AGENT_TYPE_LAYER_TAR_GZ]
    )]
    fn test_local_artifact_to_agent_type_success(#[case] layer_media_types: Vec<&str>) {
        let layers = layer_media_types
            .iter()
            .map(|media_type| OciDescriptor {
                media_type: media_type.to_string(),
                ..Default::default()
            })
            .collect();
        let manifest = OciImageManifest {
            artifact_type: Some(ManifestArtifactType::AgentType.to_string()),
            layers,
            ..Default::default()
        };

        let layer = LocalAgentType::get_layer(&manifest).unwrap();
        assert_eq!(layer.media_type, AGENT_TYPE_LAYER_TAR_GZ);
    }

    #[rstest::rstest]
    #[case::invalid_artifact_type(
        AGENT_PACKAGE_ARTIFACT_TYPE,
        vec![AGENT_TYPE_LAYER_TAR_GZ],
        "artifact type is supported"
    )]
    #[case::no_supported_layer(
        AGENT_TYPE_ARTIFACT_TYPE,
        vec!["application/vnd.custom.extra.v1"],
        "must have one supported layer"
    )]
    #[case::empty_layers(
        AGENT_TYPE_ARTIFACT_TYPE,
        vec![],
        "must have one supported layer"
    )]
    #[case::multiple_supported_layers(
        AGENT_TYPE_ARTIFACT_TYPE,
        vec![AGENT_TYPE_LAYER_TAR_GZ, AGENT_TYPE_LAYER_TAR_GZ],
        "must have exactly one supported layer"
    )]
    fn test_local_artifact_to_agent_type_failure(
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
        assert_matches!(LocalAgentType::get_layer(&manifest), Err(DefinitionError(msg)) => {
            assert!(msg.contains(expected_error), "{msg}");
        });
    }

    #[test]
    fn test_agent_type_extract_definition_success() {
        const CONTENT: &[u8] = b"namespace: newrelic\nname: com.newrelic.infrastructure\n";
        let blob = tar_gz_bytes(&[("host-linux-com.newrelic.infrastructure-0.1.0.yaml", CONTENT)]);

        let definition = LocalAgentType::new(blob).extract_definition().unwrap();

        assert_eq!(definition, CONTENT);
    }

    #[rstest::rstest]
    #[case::no_files(vec![], "does not contain any file")]
    #[case::multiple_files(
        vec![("a.yaml", b"a".as_slice()), ("b.yaml", b"b".as_slice())],
        "must contain exactly one file"
    )]
    fn test_agent_type_extract_definition_invalid_content(
        #[case] files: Vec<(&str, &[u8])>,
        #[case] expected_error: &str,
    ) {
        let blob = tar_gz_bytes(&files);

        assert_matches!(LocalAgentType::new(blob).extract_definition(), Err(DefinitionError(msg)) => {
            assert!(msg.contains(expected_error), "{msg}");
        });
    }

    #[test]
    fn test_agent_type_extract_definition_invalid_archive() {
        let blob = b"this is not a valid tar.gz".to_vec();

        assert_matches!(LocalAgentType::new(blob).extract_definition(), Err(DefinitionError(msg)) => {
            assert!(msg.contains("agent type artifact"), "{msg}");
        });
    }
}
