use std::fmt::Display;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(String);

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

/// Builds a `map_err` adapter that prefixes the source error with what was being done.
pub fn ctx<E: Display>(what: &str) -> impl FnOnce(E) -> Error + '_ {
    move |err| Error(format!("{what}: {err}"))
}
