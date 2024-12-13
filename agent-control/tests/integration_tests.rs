// This is not used in onhost code yet, so we ignore warnings for now
#[cfg_attr(feature = "onhost", allow(dead_code))]
mod common;

#[cfg(all(feature = "onhost", feature = "k8s", not(feature = "ci")))]
compile_error!("Feature \"onhost\" and feature \"k8s\" cannot be enabled at the same time");

#[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
compile_error!("Either feature \"onhost\" or feature \"k8s\" must be enabled");

#[cfg(feature = "onhost")]
mod on_host;

#[cfg(feature = "k8s")]
mod k8s;
