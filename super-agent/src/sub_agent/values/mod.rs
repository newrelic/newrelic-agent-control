pub mod values_repository;

#[cfg(feature = "k8s")]
mod k8s;
#[cfg(feature = "onhost")]
mod on_host;

#[cfg(feature = "k8s")]
pub use k8s::config_map::{ValuesRepositoryConfigMap, ValuesRepositoryError};

#[cfg(feature = "onhost")]
pub use on_host::file::{ValuesRepositoryError, ValuesRepositoryFile, FILE_PERMISSIONS};
