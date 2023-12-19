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
pub struct FSFileReader {}

#[cfg_attr(test, mockall::automock)]
impl FSFileReader {
    pub fn read(&self, file_path: &Path) -> Result<String, FileReaderError> {
        if !file_path.is_file() {
            return Err(FileReaderError::FileNotFound(format!(
                "{}",
                file_path.display()
            )));
        }
        match read_to_string(file_path) {
            Err(e) => Err(FileReaderError::Read(e)),
            Ok(content) => Ok(content),
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use mockall::predicate;
    use std::io::{Error, ErrorKind};
    use std::path::PathBuf;

    impl MockFSFileReader {
        pub fn should_read(&mut self, path: &Path, content: String) {
            self.expect_read()
                .with(predicate::eq(PathBuf::from(path)))
                .times(1)
                .returning(move |_| Ok(content.clone()));
        }

        pub fn should_not_read_file_not_found(&mut self, path: &Path, error_message: String) {
            self.expect_read()
                .with(predicate::eq(PathBuf::from(path)))
                .once()
                .returning(move |_| Err(FileReaderError::FileNotFound(error_message.clone())));
        }

        pub fn should_not_read_io_error(&mut self, path: &Path) {
            self.expect_read()
                .with(predicate::eq(PathBuf::from(path)))
                .once()
                .returning(move |_| {
                    Err(FileReaderError::Read(Error::from(
                        ErrorKind::PermissionDenied,
                    )))
                });
        }

        // the test is not idempotent as it iterates hashmap. For now let's use this
        pub fn could_read(&mut self, path: &Path, content: String) {
            self.expect_read()
                .with(predicate::eq(PathBuf::from(path)))
                .returning(move |_| Ok(content.clone()));
        }
    }

    #[test]
    fn test_file_not_found_should_return_error() {
        let reader = FSFileReader::default();
        let result = reader.read(Path::new("/a/path/that/does/not/exist"));
        assert!(result.is_err());
        assert_eq!(
            String::from("file not found: `/a/path/that/does/not/exist`"),
            result.unwrap_err().to_string()
        );
    }
}
