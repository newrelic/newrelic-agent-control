use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct AWSMetadata {
    #[serde(rename = "instanceId")]
    pub(super) instance_id: String,
}
