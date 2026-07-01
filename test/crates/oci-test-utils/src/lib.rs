use aws_lc_rs::digest::{SHA256, digest};

pub mod agent_type_meta;
mod publisher;
mod signer;

pub use agent_type_meta::{AgentTypeDefinitionMeta, MetaError};
pub use publisher::{AgentTypeArtifact, ArtifactKind, PackageMediaType, PackagePublisher};
pub use signer::OCISigner;

/// Port to be used for plain http testing registries
pub const LOCAL_HTTP_REGISTRY_URL: &str = "localhost:5001";

pub fn blob_digest(data: &[u8]) -> String {
    format!("sha256:{}", hex_bytes(digest(&SHA256, data).as_ref()))
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
