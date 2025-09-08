use std::fmt::Debug;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("`{0}` not piped")]
    StreamPipeError(String),

    #[error("`{0}`")]
    IOError(#[from] std::io::Error),

    #[error("Nix Error: `{0}`")]
    NixError(String),
}
