pub mod values_repository;

// TODO to be moved below onhost cfg flag when k8s implementation is ready.
mod on_host;
pub use on_host::file::{ValuesRepositoryFile, FILE_PERMISSIONS};
