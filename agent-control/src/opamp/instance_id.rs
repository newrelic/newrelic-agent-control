pub mod definition;
pub mod getter;
pub mod storer;

pub use definition::InstanceID;

#[cfg(feature = "k8s")]
pub mod k8s;
#[cfg(feature = "onhost")]
pub mod on_host;
