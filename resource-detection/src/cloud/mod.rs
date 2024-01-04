//! Cloud instances resource detector
pub mod aws;
pub mod azure;
/// HTTP Client used by cloud detectors
pub mod http_client;

/// AWS_INSTANCE_ID represents the key attribute
pub const AWS_INSTANCE_ID: &str = "aws_instance_id";
/// AZURE_INSTANCE_ID represents the key attribute
pub const AZURE_INSTANCE_ID: &str = "azure_instance_id";
