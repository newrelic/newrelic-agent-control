pub mod directory_manager;
pub mod file_deleter;
pub mod file_reader;
pub mod file_renamer;
pub mod utils;
pub mod win_permissions;
pub mod writer_file;

#[derive(Debug)]
pub struct LocalFile;

#[cfg(feature = "mocks")]
pub mod mock {
    use std::io;
    use std::path::Path;
    use std::path::PathBuf;

    use super::file_deleter::FileDeleter;
    use super::file_reader::{FileReader, FileReaderError};
    use super::file_renamer::{FileRenamer, FileRenamerError};
    use super::writer_file::{FileWriter, WriteError};
    use mockall::mock;

    mock! {
        pub LocalFile {}

        impl FileReader for LocalFile {
            fn read(&self, file_path: &Path) -> Result<String, FileReaderError>;
            fn dir_entries(&self, dir_path: &Path) -> Result<Vec<PathBuf>, FileReaderError>;
        }

        impl FileRenamer for LocalFile {
            fn rename(&self, file_path: &Path, rename_path: &Path) -> Result<(), FileRenamerError>;
        }

        impl FileWriter for LocalFile {
            fn write(
                &self,
                path: &Path,
                buf: String,
            ) -> Result<(), WriteError>;
        }

        impl FileDeleter for LocalFile {
            fn delete(&self, file_path: &Path) -> io::Result<()>;
        }
    }
}
