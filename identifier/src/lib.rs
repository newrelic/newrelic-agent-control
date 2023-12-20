use system::detector::SystemDetectorError;

mod file_reader;
pub mod system;

pub struct Resource<const N: usize> {
    // Set of attributes that describe the resource.
    // Attribute keys MUST be unique (it is not allowed to have more than one
    // attribute with the same key).
    pub attributes: [(String, Result<String, DetectError>); N],
}

#[derive(thiserror::Error, Debug)]
pub enum DetectError {
    #[error("error detecting system resources `{0}`")]
    SystemError(#[from] SystemDetectorError),
}

pub trait Detect<const N: usize> {
    fn detect(&self) -> Resource<N>;
}
