use serde::Deserialize;

pub(crate) const IPV4_METADATA_ENDPOINT: &str = konst::option::unwrap_or!(
    option_env!("TEST_IPV4_METADATA_ENDPOINT"),
    "http://169.254.169.254/latest/dynamic/instance-identity/document"
);

#[derive(Deserialize)]
pub(super) struct AWSMetadata {
    #[serde(rename = "instanceId")]
    pub(super) instance_id: String,
}
