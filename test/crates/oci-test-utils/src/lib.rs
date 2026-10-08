use aws_lc_rs::digest::{SHA256, digest};
use oci_client::config::{Architecture, Os};

pub mod agent_type_meta;
mod mirror;
mod publisher;
mod signer;

pub use agent_type_meta::{AgentTypeDefinitionMeta, MetaError};
pub use mirror::mirror_host_package;
pub use publisher::{AgentTypeArtifact, ArtifactKind, PackageMediaType, PackagePublisher};
pub use signer::OCISigner;

/// Port to be used for plain http testing registries
pub const LOCAL_HTTP_REGISTRY_URL: &str = "localhost:5001";

/// Platform of the packages Agent Control resolves. Windows only ships amd64 builds, which also
/// run emulated on arm64 hosts, so packages are published as amd64 whatever the host is.
pub fn package_platform() -> (Os, Architecture) {
    if cfg!(windows) {
        (Os::Windows, Architecture::Amd64)
    } else {
        (Os::default(), Architecture::default())
    }
}

pub fn blob_digest(data: &[u8]) -> String {
    format!("sha256:{}", hex_bytes(digest(&SHA256, data).as_ref()))
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
