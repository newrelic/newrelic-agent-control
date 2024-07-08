use thiserror::Error;

#[derive(Debug, Error)]
pub enum EffectiveConfigError {
    #[error(transparent)]
    Loader(LoaderError),
}

/// Error type for the effective configuration loader.
/// This is implementation-dependent so it only encapsulates a String.
#[derive(Debug, Error)]
#[error("could not load effective configuration: `{0}`")]
pub struct LoaderError(String);

impl From<String> for LoaderError {
    fn from(s: String) -> Self {
        LoaderError(s.to_string())
    }
}
