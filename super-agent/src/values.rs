#[cfg(feature = "k8s")]
pub mod k8s;
// TODO Change name. This is used by k8s as well since at startup time we read the SA config from the disk
pub mod on_host;
pub mod values_repository;
pub mod yaml_config;
