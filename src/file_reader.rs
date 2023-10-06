use std::fs::read_to_string;
use std::io::Error as ioError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FileReaderError {
    #[error("error reading contents: `{0}`")]
    Read(#[from] ioError),
}

pub trait FileReader {
    fn read(&self, path: &str) -> Result<String, FileReaderError>;
}

#[derive(Default)]
pub struct FSFileReader;

impl FileReader for FSFileReader {
    fn read(&self, path: &str) -> Result<String, FileReaderError> {
        match read_to_string(path) {
            Err(e) => Err(FileReaderError::Read(e)),
            Ok(content) => Ok(content),
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use mockall::{mock, predicate};

    mock! {
        pub FileReaderMock {}

        impl FileReader for FileReaderMock {
            fn read(&self, path:&str) -> Result<String, FileReaderError>;
        }
    }

    impl MockFileReaderMock {
        pub fn should_read(&mut self, path: String, content: String) {
            self.expect_read()
                .with(predicate::eq(path.clone()))
                .times(1)
                .returning(move |_| Ok(content.clone()));
        }

        // the test is not idempotent as it iterates hashmap. For now let's use this
        pub fn could_read(&mut self, path: String, content: String) {
            self.expect_read()
                .with(predicate::eq(path.clone()))
                .returning(move |_| Ok(content.clone()));
        }
    }
}
