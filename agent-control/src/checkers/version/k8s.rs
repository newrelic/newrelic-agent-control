//! Kubernetes version checkers.
/// The aggregate k8s version checker and its spawning thread.
pub mod checkers;
/// Version extraction from a Flux HelmRelease resource.
pub mod helmrelease;
/// Version extraction from a New Relic Instrumentation resource.
pub mod instrumentation;
