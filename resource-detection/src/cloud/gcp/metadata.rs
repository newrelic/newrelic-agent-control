use serde::Deserialize;
use serde_json::Number;

#[derive(Deserialize)]
pub(super) struct GCPMetadata {
    #[serde(rename = "id")]
    pub(super) instance_id: Number,
}
