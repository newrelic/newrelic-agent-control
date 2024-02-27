pub mod repository;
pub use repository::HashRepository;

// TODO to be moved below onhost cfg flag when k8s implementation is ready.
#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
mod k8s;
#[cfg(feature = "onhost")]
mod on_host;

#[cfg(all(not(feature = "onhost"), feature = "k8s"))]
pub use k8s::config_map::{HashRepositoryConfigMap, HashRepositoryError};

#[cfg(feature = "onhost")]
pub use on_host::file::{HashRepositoryError, HashRepositoryFile};
