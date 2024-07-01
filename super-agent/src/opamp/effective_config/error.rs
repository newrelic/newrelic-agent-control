use thiserror::Error;

use super::loader::LoaderError;

#[derive(Debug, Error)]
pub enum EffectiveConfigError {
    #[error(transparent)]
    Loader(LoaderError),
}
