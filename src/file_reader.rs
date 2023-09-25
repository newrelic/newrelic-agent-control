use std::fs::read_to_string;
use std::io::Error as ioError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FileReaderError {
    #[error("error reading contents: `{0}`")]
    Read(#[from] ioError),
}

pub trait FileReader {
    fn read(&self, path: &String) -> Result<String, FileReaderError>;
}

#[derive(Default)]
pub struct FSFileReader;

impl FSFileReader {
    pub fn new() -> Self {
        Self::default()
    }
}
impl FileReader for FSFileReader {
    fn read(&self, path: &String) -> Result<String, FileReaderError> {
        match read_to_string(path) {
            Err(e) => Err(FileReaderError::Read(e)),
            Ok(content) => Ok(content),
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use mockall::mock;

    mock! {
        pub FileReaderMock {}

        impl FileReader for FileReaderMock {
            fn read(&self, path:&String) -> Result<String, FileReaderError>;
        }
    }
}
