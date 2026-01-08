use crate::LocalFile;
use std::fs::remove_file;
use std::io;
use std::path::Path;

pub trait FileDeleter {
    fn delete(&self, file_path: &Path) -> io::Result<()>;
}

impl FileDeleter for LocalFile {
    fn delete(&self, file_path: &Path) -> io::Result<()> {
        if !file_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{}", file_path.display()),
            ));
        }

        remove_file(file_path)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_delete_not_found_should_return_error() {
        let deleter = LocalFile;
        let result = deleter.delete(Path::new("/a/path/that/does/not/exist"));
        assert!(result.is_err());
        assert_eq!(
            String::from("/a/path/that/does/not/exist"),
            result.unwrap_err().to_string()
        );
    }
}
