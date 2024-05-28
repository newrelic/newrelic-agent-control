pub mod getter;
pub mod storer;

pub use getter::InstanceID;

#[cfg(feature = "k8s")]
mod k8s;
#[cfg(feature = "onhost")]
mod on_host;

#[cfg(feature = "k8s")]
pub use k8s::getter::*;
#[cfg(feature = "k8s")]
pub use k8s::storer::*;

#[cfg(feature = "onhost")]
pub use on_host::getter::*;
#[cfg(feature = "onhost")]
pub use on_host::storer::*;
