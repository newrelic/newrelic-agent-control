use aws_lc_rs::digest::{SHA256, digest};

mod publisher;
mod signer;

pub use publisher::{PackageMediaType, PackagePublisher};
pub use signer::OCISigner;

pub fn blob_digest(data: &[u8]) -> String {
    format!("sha256:{}", hex_bytes(digest(&SHA256, data).as_ref()))
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
