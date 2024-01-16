use std::fs::{read_dir, read_to_string};
use std::io::Error as ioError;
use std::path::Path;
use thiserror::Error;

use super::LocalFile;

#[derive(Error, Debug)]
pub enum FileReaderError {
    #[error("error reading contents: `{0}`")]
    Read(#[from] ioError),
    #[error("file not found: `{0}`")]
    FileNotFound(String),
    #[error("dir not found: `{0}`")]
    DirNotFound(String),
}

pub trait FileReader {
    fn read(&self, file_path: &Path) -> Result<String, FileReaderError>;
    fn read_dir(&self, dir_path: &Path) -> Result<Vec<String>, FileReaderError>;
}

impl FileReader for LocalFile {
    fn read(&self, file_path: &Path) -> Result<String, FileReaderError> {
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

    fn read_dir(&self, dir_path: &Path) -> Result<Vec<String>, FileReaderError> {
        if !dir_path.is_dir() {
            return Err(FileReaderError::DirNotFound(format!(
                "{}",
                dir_path.display()
            )));
        }
        let files = read_dir(dir_path)?;
        let mut file_paths: Vec<String> = Vec::new();
        for path in files {
            file_paths.push(path?.path().into_os_string().into_string().unwrap());
        }
        Ok(file_paths)
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Mock
////////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "mocks")]
pub mod mock {
    use super::*;
    use crate::mock::MockLocalFile;
    use mockall::predicate;
    use std::io::{Error, ErrorKind};
    use std::path::PathBuf;

    impl MockLocalFile {
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

        pub fn should_read_dir(&mut self, path: &Path, content: Vec<String>) {
            self.expect_read_dir()
                .with(predicate::eq(PathBuf::from(path)))
                .times(1)
                .returning(move |_| Ok(content.clone()));
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    fn test_file_not_found_should_return_error() {
        let reader = LocalFile::default();
        let result = reader.read(Path::new("/a/path/that/does/not/exist"));
        assert!(result.is_err());
        assert_eq!(
            String::from("file not found: `/a/path/that/does/not/exist`"),
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_dir_not_found_should_return_error() {
        let reader = LocalFile::default();
        let result = reader.read_dir(Path::new("/a/path/that/does/not/exist"));
        assert!(result.is_err());
        assert_eq!(
            String::from("dir not found: `/a/path/that/does/not/exist`"),
            result.unwrap_err().to_string()
        );
    }
}
