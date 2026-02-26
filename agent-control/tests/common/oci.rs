use aws_lc_rs::digest::SHA256;
use aws_lc_rs::digest::digest;
use oci_client::Client;
use oci_client::Reference;
use oci_client::config::Architecture;
use oci_client::config::Os;
use oci_client::manifest::{IMAGE_CONFIG_MEDIA_TYPE, OciDescriptor};

pub async fn push_empty_config_descriptor(
    oci_client: &Client,
    reference: &Reference,
) -> OciDescriptor {
    let empty_config = b"{}";
    let digest = oci_blob_digest(empty_config);

    oci_client
        .push_blob(reference, empty_config.as_slice(), digest.as_str())
        .await
        .unwrap();

    OciDescriptor {
        media_type: "application/vnd.oci.empty.v1+json".to_string(),
        digest,
        size: empty_config.len() as i64,
        ..Default::default()
    }
}

pub fn oci_blob_digest(data: &[u8]) -> String {
    format!("sha256:{}", hex_bytes(digest(&SHA256, data).as_ref()))
}

/// Pushes a platform config blob containing `{"architecture":"<arch>","os":"<os>"}` and returns
/// its descriptor with the standard OCI image config media type.
pub async fn push_platform_config_descriptor(
    oci_client: &Client,
    reference: &Reference,
) -> OciDescriptor {
    let config_bytes: Vec<u8> = serde_json::to_vec(&serde_json::json!({
        "architecture": &Architecture::default(),
        "os":  &Os::default(),
    }))
    .unwrap();

    let config_digest = oci_blob_digest(&config_bytes);
    let config_size = config_bytes.len() as i64;

    oci_client
        .push_blob(reference, config_bytes, config_digest.as_str())
        .await
        .unwrap();

    OciDescriptor {
        media_type: IMAGE_CONFIG_MEDIA_TYPE.to_string(),
        digest: config_digest,
        size: config_size,
        ..Default::default()
    }
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
