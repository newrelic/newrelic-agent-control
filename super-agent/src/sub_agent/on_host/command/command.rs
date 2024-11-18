use std::fmt::Debug;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("`{0}` not piped")]
    StreamPipeError(String),

    #[error("`{0}`")]
    IOError(#[from] std::io::Error),

    #[cfg(target_family = "unix")]
    #[error("`{0}`")]
    NixError(#[from] nix::Error),
}
