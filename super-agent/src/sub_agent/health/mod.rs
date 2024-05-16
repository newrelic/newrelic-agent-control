pub mod health_checker;
#[cfg(feature = "onhost")]
pub mod on_host;

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
pub mod k8s;
