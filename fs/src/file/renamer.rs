use super::LocalFile;
use std::fs::rename;
use std::io;
use std::path::Path;

pub trait FileRenamer {
    fn rename(&self, file_path: &Path, rename_path: &Path) -> io::Result<()>;
}

////////////////////////////////////////////////////////////////////////////////////
// Mock
////////////////////////////////////////////////////////////////////////////////////
impl FileRenamer for LocalFile {
    fn rename(&self, file_path: &Path, rename_path: &Path) -> io::Result<()> {
        if !file_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("file or dir not found: {}", file_path.display()),
            ));
        }

        rename(file_path, rename_path)
    }
}

#[cfg(feature = "mocks")]
pub mod mock {
    use crate::mock::MockLocalFile;
    use mockall::predicate;
    use std::path::{Path, PathBuf};

    impl MockLocalFile {
        pub fn should_rename(&mut self, path: &Path, rename: &Path) {
            self.expect_rename()
                .with(
                    predicate::eq(PathBuf::from(path)),
                    predicate::eq(PathBuf::from(rename)),
                )
                .times(1)
                .returning(move |_, _| Ok(()));
        }
    }
}

#[cfg(test)]
pub mod tests {

    use super::*;

    #[test]
    fn test_path_not_found_should_return_error() {
        let renamer = LocalFile;
        let result = renamer.rename(
            Path::new("/a/path/that/does/not/exist"),
            Path::new("/another/path"),
        );
        assert!(result.is_err());
        assert_eq!(
            String::from("file or dir not found: /a/path/that/does/not/exist"),
            result.unwrap_err().to_string()
        );
    }
}
