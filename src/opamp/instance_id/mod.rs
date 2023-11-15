pub mod getter;
pub mod storer;

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
mod getter_k8s;
#[cfg(feature = "onhost")]
mod getter_onhost;
#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
mod storer_k8s;
#[cfg(feature = "onhost")]
mod storer_onhost;

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
pub use getter_k8s::*;
#[cfg(feature = "onhost")]
pub use getter_onhost::*;
#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
pub use storer_k8s::*;
#[cfg(feature = "onhost")]
pub use storer_onhost::*;
