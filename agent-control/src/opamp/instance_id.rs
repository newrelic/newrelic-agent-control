//! OpAMP instance identifiers: their definition, persistence, retrieval, and per-platform identifiers.
pub mod definition;
pub mod getter;
pub mod storer;

pub use definition::InstanceID;

pub mod k8s;

pub mod on_host;
