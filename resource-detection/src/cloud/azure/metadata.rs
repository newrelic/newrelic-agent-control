use serde::Deserialize;

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
