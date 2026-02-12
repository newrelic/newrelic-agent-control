use oci_client::Client;
use oci_client::Reference;
use oci_client::manifest::OciDescriptor;
use ring::digest::SHA256;
use ring::digest::digest;

pub async fn push_empty_config_descriptor(
    oci_client: &Client,
    reference: &Reference,
) -> OciDescriptor {
    let empty_config = b"{}";
    let empty_config_digest = format!(
        "sha256:{}",
        hex_bytes(digest(&SHA256, empty_config).as_ref())
    );

    oci_client
        .push_blob(
            reference,
            empty_config.as_slice(),
            empty_config_digest.as_str(),
        )
        .await
        .unwrap();

    OciDescriptor {
        media_type: "application/vnd.oci.empty.v1+json".to_string(),
        digest: empty_config_digest.clone(),
        size: empty_config.len() as i64,
        ..Default::default()
    }
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
