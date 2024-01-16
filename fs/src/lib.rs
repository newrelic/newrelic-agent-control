pub mod directory_manager;
pub mod file_reader;
pub mod file_renamer;
pub mod utils;
pub mod writer_file;

#[derive(Debug, Default)]
pub struct LocalFile;

pub mod mock {
    use std::{fs::Permissions, path::Path};

    use super::file_reader::{FileReader, FileReaderError};
    use super::file_renamer::{FileRenamer, FileRenamerError};
    use super::writer_file::{FileWriter, WriteError};
    use mockall::mock;

    mock! {
        pub LocalFile {}

        impl FileReader for LocalFile {
            fn read(&self, file_path: &Path) -> Result<String, FileReaderError>;
            fn read_dir(&self, dir_path: &Path) -> Result<Vec<String>, FileReaderError>;
        }

        impl FileRenamer for LocalFile {
            fn rename(&self, file_path: &Path, rename_path: &Path) -> Result<(), FileRenamerError>;
        }

        impl FileWriter for LocalFile {
            fn write(
                &self,
                path: &Path,
                buf: String,
                permissions: Permissions,
            ) -> Result<(), WriteError>;
        }
    }
}
