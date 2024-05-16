pub mod health_checker;
#[cfg(feature = "onhost")]
pub mod on_host;
#[cfg(feature = "onhost")]
pub use on_host::error::HealthCheckerError;

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
pub use k8s::error::HealthCheckerError;
#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
pub mod k8s;
