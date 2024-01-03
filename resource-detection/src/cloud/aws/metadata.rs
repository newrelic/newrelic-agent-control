use serde::Deserialize;

pub(crate) const IPV4_METADATA_ENDPOINT: &str = "http://169.254.169.254/latest/meta-data/";

#[derive(Deserialize)]
pub(super) struct AWSMetadata {
    #[serde(rename = "instanceId")]
    pub(super) instance_id: String,
}
