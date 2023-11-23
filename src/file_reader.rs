#[cfg(test)]
use mockall::automock;
use std::fs::read_to_string;
use std::io::Error as ioError;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FileReaderError {
    #[error("error reading contents: `{0}`")]
    Read(#[from] ioError),
    #[error("file not found: `{0}`")]
    FileNotFound(String),
}

#[derive(Default)]
pub struct FSFileReader;

#[cfg_attr(test, automock)]
impl FSFileReader {
    pub fn read(&self, path: &str) -> Result<String, FileReaderError> {
        let file_path = Path::new(&path);
        if !file_path.is_file() {
            return Err(FileReaderError::FileNotFound(path.to_string()));
        }
        match read_to_string(path) {
            Err(e) => Err(FileReaderError::Read(e)),
            Ok(content) => Ok(content),
        }
    }
}

#[cfg(test)]
pub mod test {

    use mockall::predicate;
    use std::io::{Error, ErrorKind};

    #[double]
    use crate::file_reader::FSFileReader;
    use crate::file_reader::FileReaderError;
    use mockall_double::double;

    impl FSFileReader {
        pub fn should_read(&mut self, path: String, content: String) {
            self.expect_read()
                .with(predicate::eq(path.clone()))
                .times(1)
                .returning(move |_| Ok(content.clone()));
        }

        pub fn should_not_read_file_not_found(&mut self, path: String, error_message: String) {
            self.expect_read()
                .with(predicate::eq(path.clone()))
                .once()
                .returning(move |_| Err(FileReaderError::FileNotFound(error_message.clone())));
        }

        pub fn should_not_read_io_error(&mut self, path: String) {
            self.expect_read()
                .with(predicate::eq(path.clone()))
                .once()
                .returning(move |_| {
                    Err(FileReaderError::Read(Error::from(
                        ErrorKind::PermissionDenied,
                    )))
                });
        }

        // the test is not idempotent as it iterates hashmap. For now let's use this
        pub fn could_read(&mut self, path: String, content: String) {
            self.expect_read()
                .with(predicate::eq(path.clone()))
                .returning(move |_| Ok(content.clone()));
        }
    }
}
