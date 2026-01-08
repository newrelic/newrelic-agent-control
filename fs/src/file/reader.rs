use super::LocalFile;
use std::fs::{self, read_dir};
use std::io;
use std::path::{Path, PathBuf};

pub trait FileReader {
    /// Read the contents of file_path and return them as string.
    ///
    /// If the file is not present it will return a FileReaderError
    fn read(&self, file_path: &Path) -> io::Result<String>;

    /// Return the entries inside a given Path.
    ///
    /// If the path does not exist it will return a FileReaderError
    fn dir_entries(&self, dir_path: &Path) -> io::Result<Vec<PathBuf>>;
}

impl FileReader for LocalFile {
    fn read(&self, file_path: &Path) -> io::Result<String> {
        if !file_path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("file not found or not a file: {}", file_path.display()),
            ));
        }

        let file_contents = fs::read(file_path)?;

        match str::from_utf8(&file_contents) {
            Ok(s) => Ok(s.to_string()),
            #[cfg(target_family = "unix")]
            Err(e) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("UTF-8 decoding error: {e}"),
            )),
            #[cfg(target_family = "windows")]
            Err(_) => fallback_decode_windows_1252(&file_contents),
        }
    }

    fn dir_entries(&self, dir_path: &Path) -> io::Result<Vec<PathBuf>> {
        if !dir_path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "directory not found or not a directory: {}",
                    dir_path.display()
                ),
            ));
        }
        let files = read_dir(dir_path)?;
        let mut file_paths: Vec<PathBuf> = Vec::new();
        for path in files {
            file_paths.push(path?.path());
        }
        Ok(file_paths)
    }
}

#[cfg(target_family = "windows")]
/// Fallback function that decodes data assuming Windows-1252 encoding.
/// Used if UTF-8 assumptions about the input file fail.
fn fallback_decode_windows_1252(data: &[u8]) -> io::Result<String> {
    let (output, encoding_used, errors_happened) = encoding_rs::WINDOWS_1252.decode(data);
    // Emit the actual encoding used, which might vary form the attempted due to BOM sniffing
    // Ref: <https://docs.rs/encoding_rs/latest/encoding_rs/struct.Encoding.html#method.decode>
    tracing::debug!("Decoded using: {}", encoding_used.name());
    if errors_happened {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "UTF-8 and Windows-1252 decoding errors, file may be corrupted",
        ))
    } else {
        Ok(output.to_string())
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

    use std::path::PathBuf;

    impl MockLocalFile {
        pub fn should_read(&mut self, path: &Path, content: String) {
            self.expect_read()
                .with(predicate::eq(PathBuf::from(path)))
                .once()
                .returning(move |_| Ok(content.clone()));
        }

        pub fn should_dir_entries(&mut self, path: &Path, content: Vec<PathBuf>) {
            self.expect_dir_entries()
                .with(predicate::eq(PathBuf::from(path)))
                .once()
                .returning(move |_| Ok(content.clone()));
        }

        pub fn should_not_read_file_not_found(&mut self, path: &Path, error_message: String) {
            self.expect_read()
                .with(predicate::eq(PathBuf::from(path)))
                .once()
                .returning(move |_| {
                    Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        error_message.clone(),
                    ))
                });
        }

        pub fn should_not_read_io_error(&mut self, path: &Path) {
            self.expect_read()
                .with(predicate::eq(PathBuf::from(path)))
                .once()
                .returning(|_| {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "permission denied",
                    ))
                });
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_file_not_found_should_return_error() {
        let reader = LocalFile;
        let result = reader.read(Path::new("/a/path/that/does/not/exist"));
        assert!(result.is_err());
        assert_eq!(
            String::from("file not found or not a file: /a/path/that/does/not/exist"),
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_dir_not_found_should_return_error() {
        let reader = LocalFile;
        let result = reader.dir_entries(Path::new("/a/path/that/does/not/exist"));
        assert!(result.is_err());
        assert_eq!(
            String::from("directory not found or not a directory: /a/path/that/does/not/exist"),
            result.unwrap_err().to_string()
        );
    }
}
