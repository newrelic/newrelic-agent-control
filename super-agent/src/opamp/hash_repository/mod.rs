pub mod repository;
pub use repository::HashRepository;

// TODO to be moved below onhost cfg flag when k8s implementation is ready.
mod on_host;
pub use on_host::file::HashRepositoryError;
pub use on_host::file::HashRepositoryFile;
