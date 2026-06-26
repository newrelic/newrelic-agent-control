//! Kubernetes GUID checker: retrieves the entity GUID from k8s resources.
/// The GUID checker that selects the matching k8s resource and reports its GUID.
pub mod checker;
/// Per-resource GUID extraction implementations.
pub mod resources;
