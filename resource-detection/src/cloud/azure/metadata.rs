use serde::Deserialize;

pub(crate) const IPV4_METADATA_ENDPOINT: &str =
    "http://169.254.169.254/metadata/instance?api-version=2021-02-01";

#[derive(Deserialize)]
pub(super) struct AzureMetadataCompute {
    #[serde(rename = "vmId")]
    pub(super) instance_id: String,
}

#[derive(Deserialize)]
pub(super) struct AzureMetadata {
    // #[serde(flatten)]
    pub(super) compute: AzureMetadataCompute,
}
