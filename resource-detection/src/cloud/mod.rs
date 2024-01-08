//! Cloud instances resource detector
pub mod aws;
pub mod azure;

pub mod cloud_id;

/// HTTP Client used by cloud detectors
pub mod http_client;

/// AWS_INSTANCE_ID represents the key attribute
pub const AWS_INSTANCE_ID: &str = "aws_instance_id";
/// AZURE_INSTANCE_ID represents the key attribute
pub const AZURE_INSTANCE_ID: &str = "azure_instance_id";
/// CLOUD_INSTANCE_ID represents the key attribute for generic cloud instance id
pub const CLOUD_INSTANCE_ID: &str = "cloud_instance_id";
/// CLOUD_TYPE represents the key attribute for cloud type, ex: aws, azure
pub const CLOUD_TYPE: &str = "cloud_type";
/// CLOUD_TYPE_AWS is a constant fow aws
pub const CLOUD_TYPE_AWS: &str = "aws";
/// CLOUD_TYPE_AWS is a constant fow azure
pub const CLOUD_TYPE_AZURE: &str = "azure";
/// CLOUD_TYPE_AWS is a constant when no cloud detected
pub const CLOUD_TYPE_NO: &str = "no_cloud";
