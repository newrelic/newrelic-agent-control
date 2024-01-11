use serde::Deserialize;
use serde_json::Number;

pub(crate) const IPV4_METADATA_ENDPOINT: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/?recursive=true";

#[derive(Deserialize)]
pub(super) struct GCPMetadata {
    #[serde(rename = "id")]
    pub(super) instance_id: Number,
}
